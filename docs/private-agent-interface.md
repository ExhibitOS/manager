# Private Manager automation interface

`exhibitos-automation` is a private JSON-lines adapter to ManagerCore and the
bundled native helper. It does not expose HTTP, SQL, shell commands or raw local
authentication. This development interface is not a signed distribution claim.

Start with `exhibitos-automation --configuration /absolute/private/config.json`.
The configuration contains exactly `helper`, `profile`, `ports`, `outputRoot`,
`temporaryRoot`, and `inputs`, with optional `selectedOEX9: true` for the existing
fresh selected importer. `inputs` maps approved draft/archive input paths to their
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
- `import-oex9`: only in the explicit fresh selected mode and exact qualified
  runtime. Requires SHA-pinned archive and explicit `archiveFormat` draft.9. Reuse
  immutable native selection/job/lease/worker and selected-save handshake, retain
  the full supported receipt/idMap, drain normally and start ordinary editing.
  The durable archive request uses the caller's requestId; uncertain attempts are
  preserved and never retried. Existing success/failure profiles are refused.
- `bench-prepare`: accept the existing `add-bench` command and optional fresh
  viewpoint; write the existing frame/request packet to a new `outputPath`.
- `bench-apply`: provide its `proposalId` and a fixed shared-renderer reply at
  `inputPath` under the output root. Native checks the held request, exact reply,
  unchanged draft/CAS and narrow BenchCommand delta before persisting it.
- `save`: persist the exact native pending request before authenticated API save,
  then read back the exact Studio response and commit the native journal CAS.
  Uncertain responses retain the original request/draft/ETag. An explicitly
  repeated identical request may recover only its exact held pending revision.
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
