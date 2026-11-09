# Prepare an artwork image from the Manager command line

Use this macOS command with a Manager installation whose bundled runtime
supports ordinary image preparation. In Manager, set up the personal workspace,
create the artwork with its metadata and display rights, and open a saved,
editable exhibition. Quit Manager to release the workspace before running:

```sh
"/Applications/ExhibitOS Manager.app/Contents/MacOS/ExhibitOSManager" --image-cli
```

Use the actual path to your installed app. The command starts and owns the
bundled runtime for the same personal workspace. It reads one JSON object per
line from stdin and writes one JSON object per line to stdout. Keep stdin open
between requests and wait for each response before sending another. Close stdin
to stop the owned runtime. Shell scripts should use a subprocess with pipes.
For terminal use, keep each request on a single short line.

## Requests and responses

The first response has `version: 1`, `event: "ready"`, and a `result` containing
the workspace, tenant, principal, session capability and native document state.
Copy the four scope fields from that result into each request. The capability
belongs to the current process. For example, a read request has this shape:

```json
{"version":1,"requestId":"907af14f-e530-44cd-8c08-6ae3a3173404","operation":"read","workspaceId":"WORKSPACE","tenantId":"TENANT","principalId":"PRINCIPAL","capability":"CAPABILITY"}
```

Responses include `version`, `requestId`, and either `result` or an `error`
with a stable `code`. Use a fresh UUID for each new request and keep the
original UUID for an exact retry. Every write also includes `expectedRevision`
from the latest native document state and `expectedEditVersion` from its
`draft.document.editVersion`. The edit version is explicitly null when there
is no draft. Send `read` after a stale-state error and review the new state
before retrying. Use a new session if the current session reaches its
128-request limit.

| Operation | Fields in addition to the envelope |
| --- | --- |
| `read` | Returns the current document and session scope. |
| `import-image` | Write versions, absolute `inputPath` and `command` with `artworkId` and `artworkRevision`. |
| `artwork-status` | `command` with `artworkId`. Returns artwork, retained transfer and, when present, its current job. |
| `artwork-image-preparation-retry` | Write versions and `command` with `artworkId` and the retained `jobId`. |
| `artwork-approve` | Write versions and `command` with `artworkId`, `revision`, the reviewed full PNG `assetId` and `acknowledgeRights: true`. |
| `artwork-register` | Write versions and `command` with `artworkId`, `revision`, `roomId`, a fresh `placementId` UUID and `transform`. For example, `{"position":[0,1.55,-3.8],"rotation":[0,0,0,1],"scale":[1,1,1]}`. |
| `save` | Write versions. Persists the exact draft and verifies the stored copy. |
| `artwork-finish-reviewed` | Write versions and `command` with `artworkId`, `jobId` and `artworkRevision`. Releases the transfer slot after verifying the saved placement and approved asset. |

## Preparation, review and retry

- Select a regular local RGB8 PNG or baseline RGB8 JPEG in normal orientation.
  The source limit is 32 MiB, 8192 pixels per edge and 25,165,824 pixels total.
  The runtime validates the actual encoding. It preserves the original and
  prepares a display PNG up to 2048 pixels per edge and a smaller tier when
  needed. Each derivative has an 8 MiB cap. Manager calculates the original
  identity from the selected bytes.
- After `import-image`, poll `artwork-status` with a fresh UUID. Review the
  artwork metadata, display rights and `job.imagePreparation` receipt,
  including the original identity and full PNG. Approval requires the ready
  preparation, its drained worker and a separate explicit rights acknowledgment.
  Use the full asset ID from that receipt. Placement uses that approved asset.
- If the upload reply is uncertain, inspect the retained transfer. To resume a
  partial upload, repeat `import-image` with the same request UUID, artwork
  revision and original bytes. Once a retained job reports it can retry and its
  worker has drained, use `artwork-image-preparation-retry` with that job ID.
- If a save reply is uncertain in the same session, repeat the identical save
  request. After restarting the CLI, use `read` to inspect the pending document.
  For a retained ordinary save, use its original request UUID with the current
  document versions. Confirm the saved draft before finishing the transfer.
