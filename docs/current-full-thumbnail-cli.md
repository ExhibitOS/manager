# Current approved image thumbnails in the native CLI

The typed client uses a separately qualified producer to derive one small PNG
from the server's current approved Full image. It does not upload a caller PNG,
replace Full/coarse, revise the artwork or save an exhibition. The current older
`1b` runtime is rejected locally before any prepare POST. The producer's exact
immutable manifest/tuple must be pinned; existing model/product-image session
profiles alone are insufficient. Producer runtime assembly is independent of
this client, private-host authoring, frame work and recovery work.

The ordinary authenticated request envelope retains its selected native
revision/edit comparison and owner namespace. Commands are separate:

- `current-image-thumbnail-prepare`: `artworkId,artworkRevision`; request ID is
  forwarded with expectedRevision and nothing else in the typed creation body.
- `current-image-thumbnail-status`: `artworkId,artworkRevision,jobId`; read the
  existing import job and check its exact current approved Full and receipt.
- `current-image-thumbnail-cancel` / `current-image-thumbnail-retry`: the same
  fields; only existing supported states, same job, no automatic retry.
- `current-image-thumbnail-review`: the same fields plus a scoped outputPath.
  Validate/download the already approved asset under existing download rights,
  verify PNG hash/size/CRC/dimensions, and return an opaque review ID plus image.
  This command never selects a thumbnail.
- `current-image-thumbnail-select`: `reviewId,expectedSelectionVersion`; after
  separate operator review, recheck current Full, job, receipt, rights, issuer,
  API generation and selection version, then use the existing selection POST.

Ready requires an approved preparation job, owned worker drain and a closed,
validated server receipt/candidate. A job's approved state is not CMS artwork
approval. A pending/failed job cannot carry ready review authority. Receipt SHA,
source, ownership, rights, recipe, thumbnail identity and bounded image metrics
are checked together. Typed rights JSON differences fail closed; the client
never drops the rights binding merely to accept a receipt.

No selected native document, upload journal, publication or static-export journal
is written. Review IDs belong to the current API instance/session and cannot
supply arbitrary PNG bytes. Stale input or authority conflicts preserve the
existing job and binding for owner disposition. Source checks are not actual
producer, painting visual, binding or publication acceptance.

Qualified immutable producer tuple: `a888494bb9ebb9299b6adc14eebad314e4153573 / e662b20c775c2852521dca75cdcea1d631d4aa11d1cd157db272848d09afceaf / 0f04520a8742f9774206d12c8a2b56da8ed2a4e07c095b3cb8ad3b3e988dd988`.
Compiled route inventory SHA256: `25dc63eeb4398069de154b5b2e791795436bd7de4520035827a70c3ae155a944`.
This admission adds only the current-image thumbnail operations; other optional
runtime families retain their own gates.

Selection decodes the generic existing-asset three-field response, then reads the
current binding/version and the selected PNG under that binding to verify its
exact reviewed hash and dimensions. A lost response or readback failure remains
an uncertain mutation; there is no automatic retry. An explicit replay retains
the same request ID, candidate and expected selection version; server request
idempotency and CAS remain authoritative. Cancellation currently requires the
retained current artwork/rights context, so stale-context cleanup remains an
owner action through existing supported operations.
