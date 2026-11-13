# Ordinary private exhibitions in the native CLI

The native operator can create one independent private server exhibition in the
currently authenticated workspace without replacing the selected native draft.
The selected document, artwork upload journal and publication/export journals
are never written by these operations. No browser cookie relay, profile/workspace
creation, raw draft file import or alternate renderer is involved.

The already qualified current `1b` runtime is admitted for its existing portal
finish contract only when the authenticated session advertises portal finish
and source-bound rectangular-fit support. This does not admit other optional
material or model families or an unknown runtime.

Requests use the existing authenticated envelope, including the **selected**
document's expected revision/edit version. `private-exhibition-create` accepts
only `command: {title}`. It creates a draft through `NativeExhibitionDraft.new`
and the existing `AuthoringAPI.saveExhibition` POST, then compares a fresh GET.
Its result includes an independent in-memory `document` and `serverRevision`.

All following commands include `exhibitionId` and `privateRevision` from that
private document. They cannot select an arbitrary existing exhibition or the
selected official exhibition. Only one private host is held per operator session.

- `private-exhibition-read`: compare the remote saved baseline and report RAM state.
- `private-exhibition-reopen`: additionally require RAM to equal the saved baseline.
- `private-exhibition-edit`: include an `architecture` command from the closed
  ordinary command set below. The existing server viewport validator must accept
  the complete proposed draft before the operator adopts it in RAM.
- `private-exhibition-save`: use the existing POST/PUT and exact request ID/ETag,
  then compare a fresh GET. An uncertain save retains the same pending draft,
  request ID and ETag; a different request is rejected.
- `private-exhibition-wall-finish`: include the existing typed `wallFinish`
  inspector command. The existing saved-head producer and correspondence checker
  remain authoritative.
- `private-exhibition-portal-attach`: include `openingId`, `furnishingId`,
  `furnishingVersion: 1`, and `modelSha256`. Existing live registry, rights,
  source-bound fit and target validation still apply. Save geometry first.
- `private-exhibition-export-oex`: provide the ordinary scoped `outputPath` and
  use the existing bounded exporter. It writes no selected publication journal.

Ordinary architecture commands have closed fields:

| `type` | Additional fields |
| --- | --- |
| `room-add`, `room-update` | `id,name,width,height,depth,position` |
| `white-cube` | `roomId,surfaceIds` (six distinct stable UUIDs: front, back, left, right, floor, ceiling) |
| `surface-update` | `id,width,height,position` |
| `opening-add` | `id,surfaceId,width,height,offset` |
| `connect-openings` | `firstId,secondId` |

Coordinates are meters, right-handed Y-up. Doors use centered wall-local XY
coordinates at floor level. White cube creation requires a room without existing
surfaces; it cannot silently replace existing architecture. Connection editing
alone does not establish geometric fit: the authoritative server must validate
the actual wall/room relationship and all target/rights constraints.

The private state is intentionally confined to the live CLI session. This is not
a new durable journal or a restart-recovery facility. Keep an uncertain operation
owned and retry only its original request; never replace its ID to manufacture a
second host. Normal close refuses dirty or pending private state. A process loss
requires owner reconciliation; no automatic recreation or selected-draft switch
is supported. Existing selected-document edits remain unchanged.

Source tests and client builds do not establish actual registration, worker-issued
fit, save/reopen/output, Viewer observation or visual acceptance. Those require
separate current-owner execution receipts. No asset-specific hash, hardcoded
qualification scene, caller-authored fit receipt or legacy retrofit exemption is
introduced by this ordinary client path.

`private-exhibition-open` accepts only `exhibitionId,serverRevision,etag` from an
explicitly selected remote head. It opens that authenticated ordinary exhibition
into isolated RAM after positive ordinary policy and repeated head/policy checks,
while preserving the official native context. It accepts no caller draft or raw
snapshot and refuses the selected official exhibition. The same server-owned
exhibition can therefore be continued after a clean process exit without a
second creation. Pending RAM saves still require explicit owner reconciliation.

Architectural edits reuse the existing operator policy: positive ordinary drafts
use typed Core staging and the ordinary conditional PUT; supported bounded
provenance retains the dedicated viewport check. Missing, stale, read-only or
changing policy/head rejects staging. A FORBIDDEN reply is never a fallback.
Specialized portal/frame preparation checks are unchanged.

## Local static output of the same saved private exhibition

`private-exhibition-export-static` uses the host's existing explicit exhibition,
server revision and ETag comparison and an output directory under the owning
private output root. It reuses the ordinary saved-head readiness, local
publication and native static producer, preserving the exhibition ID. It does
not select the private draft into the official document journal.

The helper requires both the private publication's owner/session/request SQL
receipt and a separate comparison against the protected selected document.
The Swift wrapper fences the complete protected document before publication and
producer dispatch and after completion. Missing, dirty, foreign or same-selected
contexts are refused. The existing saved-head route retains its original wire
when no private context is supplied. Only the exact reviewed debug runtimes are
admitted; this does not broaden the abandoned-publication custody operation.

Local publication creates ordinary publication/request/binding/audit rows and
advances the workspace's latest native static-export status. Preserve its prior
bytes as evidence and record the exact before/after and target when using an
existing workspace. Official document/publication journals, retained detail
records, original assets and previous output directories remain protected.
A successful local output is neither an Internet deployment nor Viewer visual
or physical-clearance acceptance.
