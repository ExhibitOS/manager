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
request-selected executable. The profile must not exist; its parent must exist.
The output and temporary roots must be existing, owned, private 0700 directories.
Each role needs distinct profile, port triple, temporary root and output root.
No existing Manager profile is rebound. All fresh-profile input bytes and runtime
admission must be checked by the operator before handing over the configuration.

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
- `floor-appearance-apply`: `command` contains exactly `roomId`, `surfaceId` and
  `preset` (`wood`, `marble`, `tile` or `carpet`). Use the mapped room and
  floor IDs from the import receipt. The current saved Studio policy must
  explicitly permit ordinary editing and match the native ETag. The runtime
  must serve the verified preset preview. The existing floor reducer applies
  the preset's appearance fields for that floor, retaining its sound and
  footstep resources. The response contains the new native `document` and the
  observed saved `policy`. Save before exporting. This command has no visual or
  audio observation.
- `bench-prepare`: accept the existing `add-bench` command and optional fresh
  viewpoint; write the existing frame/request packet to a new `outputPath`.
- `bench-apply`: provide its `proposalId` and a fixed shared-renderer reply at
  `inputPath` under the output root. Native checks the held request, exact reply,
  unchanged draft/CAS and narrow BenchCommand delta before persisting it.
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
  `document` and the Studio revision. If reopening fails, close the session and
  preserve its profile.
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
