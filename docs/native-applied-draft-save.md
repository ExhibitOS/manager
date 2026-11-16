# Retain a selected scene apply for explicit Save retry

This developer SDK path is opt-in and requires an existing qualified workspace.
The ordinary configuration opener still refuses dirty documents. It does not
recover historical dirty drafts whose operation cannot be attributed.

For `pedestal-apply`, `light-apply`, `fixture-apply`, `light-attachment-apply` or
`light-object-apply`, add `retainSaveIntent: true` to the existing request.
All proposal, owner, current-version and server comparison checks still run.
When the proposal changes the draft, the single existing document write stores
the resulting draft and its exact Save intent together. The existing Pending
uses origin `automation-applied-draft-v1` and the apply request ID as the durable
server Save ID. A no-op creates no intent. Omission preserves the old behavior.

In the **same SDK session**, issue `applied-draft-save-resume` with a fresh RPC
request ID, current scope/capability/versions, and
`command: {"applyRequestId": "ORIGINAL_APPLY_UUID"}`. The SDK resolves only its
own retained original request and successful opt-in apply response. No future
input file or mutable input registry is needed, including when an external
apply reply was lost. An already-saved retry in that same session also uses the
retained exact body and returns without another local write. A no-op,
non-opt-in request or UUID from another session cannot supply this handoff.

For recovery **across sessions**, retain the caller's original intent in a
private JSON file directly inside the recovery configuration's `outputRoot`,
and pin its bytes in that new configuration's existing `inputs` map. This file
can be prepared after the original prepare/apply; it need not be known when the
original SDK session was constructed. Its exact fields are:

| Field | Original value |
| --- | --- |
| `version` | `1` |
| `requestId` | Original apply UUID, also the durable Save UUID |
| `workspaceId`, `tenantId`, `principalId` | Original document owner |
| `exhibitionId` | Original exhibition |
| `runtimeManifestSHA256` | Original qualified runtime manifest |
| `sourceNativeRevision` | Native revision before apply |
| `sourceEditVersion` | Saved edit version before apply |
| `etag` | Original server ETag before apply |
| `candidate` | Exact selected candidate returned by prepare |
| `originalDraft` (optional) | Exact AFTER draft envelope from the applied result or retained Pending |

An apply stamps `updatedAt`, so a prepared candidate does not predict its full
AFTER envelope. A retained Pending supplies that exact original body. A retry
after the final local confirmation has cleared Pending requires the caller's
exact `originalDraft`; the SDK does not reconstruct it from a candidate.

For the file route, issue `applied-draft-save-resume` with a **new RPC request
ID**, the ordinary scope/capability/current `expectedRevision` and
`expectedEditVersion`, and `inputPath` pointing to the pinned intent file.
Supply either `command` or `inputPath`, never both. Do not dispatch ordinary `save`
using the apply UUID: different RPC bytes under that UUID are a request conflict.
The explicit resume calls the existing Save API internally using the retained
original body, apply UUID and original ETag. It never reapplies the proposal.

After an interrupted session, add `appliedDraftRecoveryPath` to the existing
debug SDK configuration alongside `existingWorkspaceV5`. The path must be the
same kind of pinned private input. This existing-managed-profile opener accepts
only that exact typed Pending or the exact already-saved state; read and explicit
resume are its only business operations. Its normal ready/read response exposes
the local document to the local caller, allowing the caller to retain the exact
body before requesting Save. The standalone scalar inspector remains redacted.

Save failure preserves Pending. A committed Save whose reply was lost replays
the original server receipt. If the final local state is already clean, the
caller must supply the original body, and the current local/server body must
match it exactly. The old ETag must differ from the new one: without an original
receipt, the server rejects the stale ETag rather than creating another revision.
An already-saved result performs no local rewrite. Foreign owners, unrelated
edits, wrong versions, changed runtime or server heads are conflicts, preserving
the document. An active owner is refused through the existing profile fence;
startup diagnostics retain the bridge's generic `NATIVE_HELPER_EXITED` limitation.

The helper rejects modifications to this Pending except its exact successful
Save transition. Generic App/SDK Save cannot consume this tagged origin. Older
helpers reject the new origin, so a qualified matching SDK/helper is required
before opting in. No new journal, server API, raw profile reader or automatic
dirty recovery is introduced. Source tests, component qualification and actual
supported-profile use remain separate evidence.
