# Private Manager automation interface

`exhibitos-automation` is a private JSON-lines adapter to ManagerCore and the
bundled native helper. It does not expose HTTP, SQL, shell commands or raw local
authentication. This development interface is not a signed distribution claim.

Start with `exhibitos-automation --configuration /absolute/private/config.json`.
The configuration contains exactly `helper`, `profile`, `ports`, `outputRoot`,
`temporaryRoot`, and `inputs`, with at most one of `selectedOEX9: true` and
`selectedOEX10: true` for the corresponding fresh selected importer.
`inputs` maps approved draft/archive/GLB input paths to their
SHA256. The helper is the operator-selected paired native executable, never a
request-selected executable. For fresh mode the profile must not exist and its
parent must exist.
The output and temporary roots must be existing, owned, private 0700 directories.
Each role needs distinct profile, port triple, temporary root and output root.
No existing Manager profile is rebound. All fresh-profile input bytes and runtime
admission must be checked by the operator before handing over the configuration.

For the isolated v5 owner qualification, a debug CLI accepts
`workspaceV5ValidationRuntime` as an absolute runtime directory with
`selectedOEX10: true`. Use a debug Rust helper built with the non-default
`workspace-v5-owner-validation` feature. The helper verifies the exact candidate
source, producer, complete manifest and capability marker before preparing a
fresh profile. The same pin is required for the inherited-FD format check and
for `reopen-saved` on that profile. These commands leave release and workspace
material admission unchanged.

To start on a saved isolated v5 profile already handed to the operator, use
the same CLI with the following debug configuration. Omit both selected
import flags. Supply the exact scope from the prior import receipt and use
the existing port assignments.

```json
{
  "helper": "/absolute/private/exhibitos-native",
  "profile": "/absolute/private/returned-profile",
  "ports": {"web": 52044, "api": 52045, "database": 52046},
  "outputRoot": "/absolute/private/output",
  "temporaryRoot": "/absolute/private/tmp",
  "inputs": {},
  "workspaceV5ValidationRuntime": "/absolute/private/runtime",
  "existingWorkspaceV5": {
    "workspaceId": "00000000-0000-4000-8000-000000000001",
    "tenantId": "00000000-0000-4000-8000-000000000002",
    "principalId": "00000000-0000-4000-8000-000000000003",
    "manifestSHA256": "40ca49e65e769c4c034eea69300ff65ce8af46ef9b081a9a2b4b1ade12ed9415"
  }
}
```

Run `exhibitos-automation --configuration /absolute/private/reopen.json`.
The existing profile must be exclusively available and its ports free.
The ordinary helper checks the prepared runtime and the CLI checks the saved
scope before starting. It emits `ready` only after the authenticated native
session confirms the material capability and the Studio draft and ETag match
the saved document. A mismatched scope or manifest refuses startup. The CLI
skips preparation and import in this mode. Use the scoped `close` request to
drain the owned runtime.
For the paired 040 local-validation workspace, use
`workspaceV7ValidationRuntime` in place of `workspaceV5ValidationRuntime`
and give `existingWorkspaceV5.manifestSHA256` the exact 040 manifest. The
`existingWorkspaceV5` key retains its name and holds the same saved scope.

The first `ready` event returns workspace, tenant, principal, a session capability
and the current native document. Keep the process and inherited pipes open. All
requests carry version1, UUID `requestId`, `operation`, and the exact four scope
fields (`workspaceId`, `tenantId`, `principalId`, `capability`) from this event.
Responses echo requestId and contain `result` or structured `error.code` only.
Short newline-terminated requests are dispatched while stdin stays open. The
reader uses bounded POSIX short reads and one-frame backpressure; partial EOF or
an oversized frame reports a protocol error, drains the owner and exits2.
Use persistent subprocess pipes for automation. An interactive terminal's canonical
line limit can truncate a valid JSON frame before the CLI receives it; an owned
PTY must use noncanonical input before sending longer frames. Do not pad a request
or close stdin to trigger dispatch.
`AUTOMATION_OEX_UNCERTAIN` retains authorized scoped request/job IDs and explicitly
forbids retry; an unavailable import journal is not reported as an absent intent.

Writes also require `expectedRevision` (native document journal CAS counter) and
`expectedEditVersion` (draft editVersion, explicitly null before loading a draft).
The native journal counter differs from Studio server revision and ETag.

Operations:

- `read`: return current native journal without modifying it.
- `status`: return active job ID and operation-task state, including while busy.
- `load-copy`: read the SHA-pinned `inputPath` and use the existing native copy
  operation. Return the changed draft/exhibition/revision identity map; preserve
  typed nested IDs and opaque fields. This is not OEX ingestion or blob transfer.
- `catalog-read`: return the existing artist/artwork first pages for a small,
  isolated catalogue. A nonempty next cursor refuses with
  `AUTOMATION_CATALOG_PAGE_LIMIT`; incomplete pages cannot reconcile a creation.
- `artist-create`: `command` contains exactly `name` and `bio`.
- `artwork-create`: `command` contains exactly `artistId` and `metadata` in the
  existing CMS `ArtworkMetadata` shape. Unknown or discarded metadata is refused.
  Both creation operations persist a private attempt before the non-idempotent
  CMS POST and a separate acknowledged result afterward. An uncertain attempt
  refuses with `AUTOMATION_CREATE_UNCERTAIN`; inspect the retained attempt/result
  and catalogue before any explicit new creation. No replacement ID is generated
  by recovery, and successful same-request replay uses the existing session cache.
- `import-glb`: requires SHA-pinned `inputPath` and exactly `artworkId`,
  `artworkRevision`, `scaleMeters` in `command`. Read the unchanged bounded GLB,
  recheck the artwork revision/rights, and use the GUI's `NativeImportTransfer`
  with the same native upload intent/job journal. The caller requestId is the
  durable import idempotency key. A different intent cannot replace a retained
  one, including after uncertain replies. No texture/model derivation is requested.
- `artwork-status`: `command` contains exactly `artworkId`; return its current
  authenticated CMS row plus retained native upload state and matching job status.
  An approved job is not an artwork approval or a native worker-drain receipt.
- `artwork-approve`: exactly `artworkId`, `revision`, `assetId`, and explicit
  `acknowledgeRights: true`. Recheck the retained job's approved asset, original
  hash/length/MIME, current artwork revision and unchanged rights before invoking
  the existing approval API and reading the result back.
- `artwork-register`: exactly `artworkId`, `revision`, `roomId`, `placementId`,
  `transform`. Fetch the approved Studio snapshot, verify its tenant/CMS revision
  and display permission, and call existing native `addApprovedArtwork`. Saved
  documents retain the existing viewport policy/ETag/bounded-edit gate. The
  explicit pose uses unit scale; unsupported bounded editing refuses. Registration
  changes only the native draft; use the existing save/export operations afterward.
- `import-oex9`: only in the explicit fresh selected mode and exact qualified
  runtime. Requires SHA-pinned archive and explicit `archiveFormat` draft.9. Reuse
  immutable native selection/job/lease/worker and selected-save handshake, retain
  the full supported receipt/idMap, drain normally and start ordinary editing.
  The durable archive request uses the caller's requestId; uncertain attempts are
  preserved and never retried. Existing success/failure profiles are refused.
- `import-oex10`: set `selectedOEX10: true` and use a new `outputPath`,
  allowlisted `inputPath` and explicit `archiveFormat: "1.0.0-draft.10"`. The
  exact runtime must qualify for selected 028/v4. The helper verifies the format
  against the SHA-pinned private archive before transfer. This shares the same
  one-shot selected-save handshake, authentic import receipt and ordinary
  editing transition as draft.9. The returned `receipt.path` holds the actual
  import `idMap` and `assetAliases`.
- `floor-appearance-apply` accepts `roomId` and `surfaceId` in `command` plus
  exactly one selection. Use `preset` (`wood`, `marble`, `tile` or `carpet`)
  for the existing presets. Use `catalog: {"id":"quiet-oak-herringbone","version":1}`
  for the named private card. The card requires a ready, qualified local
  runtime and a verified catalogue response and preview for the authenticated
  tenant. It sets the pack, 0.9 m repeat, rotation and normal scale for the
  mapped floor. Use the room and floor IDs from the import receipt. The current
  saved Studio policy must explicitly permit ordinary editing and match the
  native ETag. Presets require their verified preview. Both selections use the
  same floor reducer and retain sound and footstep resources. The response
  contains the new native `document` and the
  observed saved `policy`. Save before exporting. This command has no visual or
  audio observation.
- Workspace material intake requires a qualified native runtime and an ordinary
  authenticated owner session advertising `workspace-material-intake-v1`.
  Debug Manager accepts the exact local-validation v5 runtime from Platform
  source `742412c152037c20a4c32e5ccce8535be46822aa`, with manifest SHA-256
  `40ca49e65e769c4c034eea69300ff65ce8af46ef9b081a9a2b4b1ade12ed9415`.
  Other packages and release builds return `WORKSPACE_MATERIAL_UNAVAILABLE`.
  - `workspace-material-intake-create` takes `kind` (`floor` or `wall`),
    `name`, `rights`, `provenance`, and `maps` in `command`. The outer
    request UUID is the create idempotency key. Use the same UUID and declaration
    to recover an uncertain create response.
  - `workspace-material-intake-upload` takes `intakeId`, `role` and an
    allowlisted top-level `inputPath` for the declared PNG. The CLI verifies
    the file pin and returns the intake state with verified role receipts.
  - `workspace-material-intake-status`, `workspace-material-intake-complete`,
    `workspace-material-intake-cancel` and `workspace-material-intake-retry`
    take `intakeId`. Poll status with a new outer request UUID. After an
    uncertain mutation, read status before taking another action. Cancel is
    pending while the phase is `cancelling`. Retry only when the server reports
    `retryable`.
  - `workspace-material-list` takes `cursor` (null for the first page).
    `workspace-material-read` takes `id` and `version`.
    `workspace-material-preview` also takes `role` and returns an identity,
    byte count and SHA-256 after verifying the authorized inline PNG.
  - Registration stores a workspace material version. Surface application
    and export are separate operations.
  - `workspace-material-floor-apply` takes the current `expectedRevision` and
    `expectedEditVersion` plus a `command` such as
    `{"surfaceId":"<floor UUID>","materialId":"<material UUID>","materialVersion":1,"repeatMeters":[2,2],"rotation":0}`.
    The selected floor must exist in a clean saved draft. The command checks
    its saved Studio policy and the exact registered descriptor. It accepts
    floor materials with display permission, repeats from 0.05 to
    20 metres per axis, and rotation from −π to π radians. The result contains
    `changed` and the native `document`. An identical saved binding returns
    `changed: false` without writing a revision. Otherwise use the returned
    revision and edit version with the existing `save` command.
    Existing sampling settings survive an edit to the same material. A
    replacement that would discard those settings is refused.
  - In Manager, open the registration sheet, load the registered library,
    choose **바닥 연결 설정**, select a floor, and enter repeats and rotation
    in degrees. **바닥에 연결** stages the change. Close the sheet and save
    the exhibition. The current runtime can save the binding. Surface
    rendering and portable outputs require a runtime that supports them.
  - `workspace-material-wall-apply` uses the same command fields and saved
    Studio checks. Pass a wall surface UUID and the exact registered
    wall material. Use `save` after a change. In Manager, the corresponding
    actions are **벽 연결 설정** and **벽에 연결**.
    Wall application is available to an ordinary authenticated owner session
    on the local-validation wall preview payload from Platform source
    `f4f6625110df63258a13407cbe45334e4372ab5e`, manifest SHA-256
    `6be2d5969462e4961e9ecbd3b6417936f2f075de1fbc3626d1b4f19cde7e65ec`.
    Floor-only runtime pins keep wall application unavailable in the GUI, and
    CLI attempts return `WORKSPACE_MATERIAL_BINDING_UNSUPPORTED`.
  - The qualified local 040 debug payload supports a required color map plus
    optional normal and roughness maps for both floor and wall bindings. Both
    interfaces check display access and verify each declared map before staging
    the binding. Ambient occlusion (AO) is unavailable for surface application.
    Earlier qualified runtimes keep their color-only application rules.
- On the admitted info-stand preparation runtime, a declared owner can use
  `workspace-furnishing-intake-create` with `kind: "info-stand"` and the original
  model. The authenticated client selects `normalize-legacy-sign-texture-r1`
  for bounded eligible inputs. The runtime keeps the original and validates it
  first. It prepares a separate display model only when needed. The original
  rights and use permissions remain in force.
  - A compatible model registers unchanged. A prepared registration returns
    `preparation` with the recipe, `sourceModel`, `renderableModel` and
    `runtimeManifestSHA256`. Compare both models and the producing manifest
    when approving a newly prepared result. For every poll of that intake,
    including after reopening the owner session, pass the expected
    `command.producerManifestSHA256` to `workspace-furnishing-intake-status`.
    Registered historical receipts
    retain their producing manifest when read by a later compatible runtime.
  - Before placing an info-stand on this runtime, read its registered model
    and use `workspace-furnishing-model-check` to verify the exact display bytes.
    In `workspace-furnishing-place` pass that reviewed SHA-256 as
    `command.modelSha256` and the registered intake as `command.intakeId`.
    The placement checks that exact receipt again before applying. Save and
    reopen through the existing commands.
- A qualified owner session advertising `default-seat-proposal-v1` stages the
  catalogue default seat with an eligible workspace furnishing placement in
  the same draft edit. `workspace-default-seat-reconcile` accepts the saved
  `etag` and up to 32 existing `placementIds` to stage missing defaults on a
  clean saved head. Existing valid linked seats and viewpoints are preserved.
  Use the existing `save` operation to commit a staged change.
  The macOS debug catalog-045 default-seat payload (manifest `ca92c753…`)
  supports fresh personal startup and clean same-payload reopen. Build the
  static Viewer with its private editor and the native cover companion before
  packaging, using the Platform build recipe. The payload has its own manifest
  identity and grants no profile transition or restore admission.
- On a runtime that supports backed workspace seats, save the registered
  bench visual at scale 1 with the existing host's exact room, position and
  yaw. Call `workspace-bench-seat-attach` with the clean head's `etag`, the
  existing `benchId`, `viewpointId` and `dimensions`, and the reviewed
  `placementId`, `furnishingId`, `furnishingVersion` and `modelSha256`. Omit
  `viewpoint` so the command preserves the existing camera. Save the staged
  first link and reopen to verify it. An exact replay leaves the draft intact.
  The macOS debug catalogue 1.5 payload (manifest `f8d5933a…`) supports
  fresh personal startup and clean same-payload reopen for this workflow.
  The explicit saved-workspace transition from `1245b842…` uses an
  authenticated checkpoint of the clean saved document and retained restore
  receipt. Activation and rollback keep the original installation and restore
  proof. The catalogue 1.5 payload still requires a separate restore admission
  for direct EXB activation.
- `furnishing-walking-collision-evaluate` reads the current draft revision
  through the authenticated owner session's `furnishing-walking-collision-v1`
  capability. It returns typed target IDs, their authored defaults, and any
  explicit overrides. An omitted override inherits the default. Evaluation
  makes no draft changes and reports authored settings independently of
  whether the Viewer has loaded the relevant collision geometry.
- `furnishing-walking-collision-set` requires the current `expectedRevision`
  and `expectedEditVersion` and an exact `command` with `target: {kind, id}`
  from evaluation and `enabled: true`, `false` or `null`. A Boolean
  persists an explicit override even when it matches the default. Null clears
  that override. The authenticated proposal is validated against the draft and
  applied through the local revision check. The reply contains `changed` and
  `document`. A repeated setting makes no local write. Use `save` separately
  before reopening or exporting. The Manager inspector exposes the same three
  choices under **걷기 충돌** and uses normal undo and redo.
- `bench-prepare`: accept the existing `add-bench` command and optional fresh
  viewpoint; write the existing frame/request packet to a new `outputPath`.
- `bench-apply`: provide its `proposalId` and a fixed shared-renderer reply at
  `inputPath` under the output root. Native checks the held request, exact reply,
  unchanged draft/CAS and narrow BenchCommand delta before persisting it.
- `shared-bench-prepare`: after staging the bench host, pass `command` with
  exactly `hostId` and room-coordinate `pose` (`position`, `yaw`, `scale`), plus a
  new `outputPath`. The authenticated catalog must match the known builtin
  backless-oak-bench version 1. The packet is for the shared renderer.
- `shared-bench-apply`: pass the prepare request ID as `proposalId` and the
  shared renderer output contains an envelope; write its `reply` object as a
  private 0600 JSON file directly under the output root and pass that path as
  `inputPath`. Native validates
  the exact proposal, full candidate delta, current draft and ETag before
  staging. After a restart the held proposal is unavailable: read the current
  draft and prepare again. Use a separate `save` and verify saved readback.
- `surface-texture-prepare`: require a clean saved draft and `command` with
  exactly `type: "set-material-texture"`, `surfaceId` and the existing closed
  `texture` binding. Only a wall is supported. The actual authenticated catalogue
  must contain the requested pack/version. Preserve the wall's current scalar
  material automatically; the caller cannot clear or change that value. Write a
  new `outputPath` packet for the shared domain reducer, holding its exact native
  frame, selection, ETag and command. This is a domain proposal, not a visual
  texture-rendering acceptance.
- `surface-texture-apply`: provide the prepare operation's top-level requestId as
  `proposalId` and the checked producer's reply at `inputPath` under the output
  root. The reply contains exactly `version`, `epoch`, `sequence`, `tenantId`,
  `principalId`, `requestId`, `surfaceId`, `candidate`; the six correlation fields
  match the held inner request. Existing `SurfaceMaterialTexturePreview` checks
  the complete candidate delta before native CAS. A changed ETag, pending/dirty
  document, wrong scope/selection or unrelated candidate change refuses. Existing
  template-modified bookkeeping remains part of that domain command. Use `save`
  for the authenticated server write and exact readback afterward.
  Both wall operations require a current native viewport policy with matching
  exhibition/ETag, explicit `boundedProfile`, `boundedEditRequired: false`,
  `boundedReadOnly: false` and no editing capability. Bounded provenance with an
  ordinary-size draft is supported; bounded-edit-required and unknown policies
  refuse. These operations do not synthesize a viewport ACK or bypass a bound.
- `save`: persist the exact native pending request before authenticated API save,
  then read back the exact Studio response and commit the native journal CAS.
  Uncertain responses retain the original request/draft/ETag. An explicitly
  repeated identical request may recover only its exact held pending revision.
- `reopen-saved`: with the latest native CAS and a clean saved draft, compare
  the authenticated Studio document, stop and close the owning helper, and
  reopen that same session-created profile through the ordinary helper entry.
  The new helper must observe the same installation, complete native journal,
  port triple and runtime manifest. A new runtime job must read back the same
  saved draft and ETag. The response includes both runtime job IDs, the
  `document` and the Studio revision. The profile directory identity is checked
  across the helper handoff. If reopening fails, close the session and preserve
  its profile.
- `export-oex`: require the saved unchanged document, re-read Studio ETag/draft,
  and use the existing bounded native archive writer into a new output file.
- `export-static`: use existing native publication pending/CAS, private snapshot,
  API publication preparation and native private static exporter. No hosting or
  internet deployment is performed. Export snapshot identity may differ from the
  authoring source identity and must be recorded separately.
- `cancel` / `close`: include the active `jobId` when a task is running. Foreign
  scope and mismatched job IDs refuse before side effects. A cancelled operation
  retains uncertain writes; `operationTaskFinished` is not a worker-drain claim.
  Active-job cancel additionally requests owned native stop and reports a separate
  `runtimeDrained` observation; it does not stop another workspace. After cancel,
  this session permits read/close only, preserving the stopped profile.
  `close` waits for the task and owning helper's normal shutdown. EOF cancels the
  owned task and closes that helper; it never stops a different workspace.
  A successful `close` exits the CLI without requiring an EOF workaround.
  A helper close failure remains terminal and the CLI exits nonzero on EOF.

Identical successful requests replay their held result. Reusing a request ID with
different bytes refuses. The session bounds its request history; it does not
silently evict replay protection. All operations preserve source inputs. Old
success/failure profiles, pending requests and authority evidence remain intact.
Restart recovery of a closed automation session is not implemented by this fresh
profile entry point; preserve its profile and pending records for the owner.

The wall adapter leaves `selectedOEX9` and `import-oex9` unchanged. Its baseline
archive must be actual draft.9 from the exact approved producer. The import
receipt/idMap must establish the new wall identity; a public or prior-profile
surface ID, array label, raw public blob reference or rewritten archive version
is not a substitute. No existing success/failure profile is reopened.

For a draft.10 floor comparison, start a fresh selected session and issue
`import-oex10` with the pinned input and a new import receipt path. Read the
receipt and map the source room and floor IDs through `importReceipt.result.idMap`.
Issue `floor-appearance-apply` with the mapped IDs and `preset: "tile"` to
apply the existing Tiles074/1 defaults, including the 2.1 m repeat.
Use the latest native CAS and edit version for each of `save`, `reopen-saved`,
`export-oex` and `export-static`. Use distinct, nonexistent output paths below
the private output root. Finish with `close` and verify the returned resources.
This flow keeps explicit concrete footsteps. It produces a private comparison
and leaves visual and playback acceptance to the Viewer.

An unknown-unit GLB requires CMS `displayReference`, `sourceUnits: "unknown"`,
`scaleApplied: false`, and upload `scaleMeters: 1`. Import and approval check
the reference against the exact GLB SHA-256. The display reference describes a
provisional height. Institutional catalogue dimensions are separate, and the
mapping to GLB axes is unverified. The original GLB bytes and the approved
Artwork transform are preserved. Numeric placement supplies a unit-scale pose
and makes no floor contact, support, or facing assertion.

Registration of a qualified Artwork draft.2 promotes the editable local
Exhibition and lifecycle draft to draft.2 together. The saved Studio readback
must preserve the same candidate, including existing artworks and the approved
display-reference extension. The paired Platform producer writes OEX draft.11
and static output. The native OEX import entry points retain their existing
supported formats.

The renderer has no native profile writer. Snapshot receipts must independently
bind native workspace/identity/journal/editVersion and Studio saved revision/ETag,
plus exact OEX/static/candidate hashes. These counters must not be equated.
Concurrent workspace acceptance, actual save/export and cancellation isolation
require real execution evidence; compiling this adapter alone proves none of them.

Minimal persistent-pipe client (use the exact paired binary/configuration paths
from the operator's handoff):

```python
import json, subprocess, uuid

process = subprocess.Popen(
    [binary_path, "--configuration", configuration_path],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1,
)
ready = json.loads(process.stdout.readline())
assert ready["event"] == "ready"
scope = {key: ready["result"][key] for key in
         ("workspaceId", "tenantId", "principalId", "capability")}

def request(operation, **fields):
    message = {"version": 1, "requestId": str(uuid.uuid4()),
               "operation": operation, **scope, **fields}
    process.stdin.write(json.dumps(message) + "\n")
    process.stdin.flush()
    response = json.loads(process.stdout.readline())
    assert response["requestId"] == message["requestId"]
    return response

state = request("read")
assert "result" in state
closed = request("close")
assert closed["result"]["closed"]
assert process.wait(timeout=120) == 0  # stdin was never closed to dispatch
```

This sequential example has one outstanding request. A client issuing status or
cancel during an operation must correlate all responses by requestId and retain
the original operation response as well as the control response.

## Private native model adoption

The paired macOS helper accepts `native_model_adoption` over its existing
private host pipe, with the current `selection_token` and one closed
`adoption` action. This requires a qualified ordinary runtime with the
dedicated adoption capability. The actions run in order.

- `stage_source` takes `sourceArtworkId`, `sourceArtworkRevision`,
  `original`, `display`, and `target`. Each model identity contains
  `sha256` and `bytes`. Target identity contains `workspaceId`,
  `tenantId`, `principalId`, `artworkId`, `artworkRevision`,
  `exhibitionId`, `placementId`, and `etag`. The helper reserves both
  fixed leaves and retains the exact source grant itself.
- Stop the source normally and select the already registered target in the
  same managed helper process. Start the target and load its saved draft.
  `preview_target` takes `operationId` and the current native document
  `expectedRevision`. It returns the exact preview and `grantSha256`.
- After the target owner confirms the exact preview, `confirm_target`
  takes `operationId`, `grantSha256`, and the same `expectedRevision`.
  It checks the saved draft, artwork, placement, and runtime generation
  before the target transaction. Read the resulting saved head using the
  normal authenticated authoring path.
- `status_target` takes `operationId` and `grantSha256` to read the
  target's durable result. An unresolved status retains staging for
  reconciliation. The confirmation command requires the preview retained by
  the same runtime generation and accepts only its operation ID and digest.

- `retained_operation` returns the reserved operation ID and any available
  grant digest to the exact source or target owner. This works when source
  staging failed before a grant was returned.
- `release_precommit` takes the retained `operationId` before any target
  commit attempt. It frees the helper slot and preserves both leaves under
  their original reservation.
- `settle_target` takes `operationId`, `grantSha256`, and explicit
  `retire`. It frees the helper slot only after the exact target confirms
  a durable commit. Use `retire: true` to remove the verified reserved
  pair and `retire: false` to preserve it. If retirement is interrupted,
  retain the remaining evidence and reconcile via the target receipt.

The helper preserves reserved bytes across interruption. A new helper can
read the target receipt by its operation ID and grant digest. Retirement
requires the original helper's retained file identities. The operation
supports registered source and target selections within one managed helper
process. Separate managed profiles require an approved operator handoff.
