# Retained original-model transition

The macOS debug owner-validation helper can preserve a bound required-model
original import when moving a clean saved workspace from the exact selected-PNG
runtime to either the qualified current-image-thumbnail or Frame9 runtime.
The Frame9 path is direct; the thumbnail runtime is not an intermediate step.
Reverse, self and thumbnail-to-Frame9 pairs remain refused. The existing carried
restore and saved-document checks still apply, with the existing streamed 8 GiB
checkpoint bound. This does not admit an unqualified runtime or a recovery host.

`stage_saved`, `activate` and existing sealed-checkpoint `recover` accept the
optional closed `retainedOriginalModel` comparison. It binds the job, artwork
revision, upload operation, generation, source hash/size/scale, receipt and rights
hashes, and exact raw transfer journal hash. It is exclusive with image/GPU
retention and startup-cancellation proofs. Absent fields preserve legacy sealed
checkpoint serialization. A proof never clears the journal, retries a job,
approves CMS artwork, or places an artwork.

Before Node starts, the owned PostgreSQL query requires an approved terminal
import with no worker token, lease or historical claim. It checks exact request,
source, rights, assets, object keys and generated derivative bindings, while
requiring CMS approval and exhibition artwork membership to remain absent.
It hashes the complete job, artwork/revision, associated asset/rights/derivative
rows and tenant exhibition rows. This SQL check alone does not recompute the
canonical receipt or current API authorization/rights validity. After normal
service readiness, a read through the issued native authoring session must
return the same complete server receipt with ready/drained status; that existing
API performs canonical receipt and current access/context validation. Custody
is compared again before and after that read. The sealed checkpoint also checks
the source and display blob bytes. Any discrepancy prevents activation success.

The source includes closed-intent/checkpoint/pair tests and an opt-in predicate
test using a separate ephemeral synthetic PostgreSQL database. That fixture
exercises actual SQL evaluation; it does not replace migrations, worker/API
checks or actual current-data transition evidence. Report those separately.
