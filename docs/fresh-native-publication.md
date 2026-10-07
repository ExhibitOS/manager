# Fresh native publication qualification (T08-02 partial)

The ignored native `actual_native_publication_sigkill_and_cold_selection` test now requires an explicit `freshAuthority` object inside the existing private `EXHIBITOS_WHOLE_CRASH_INPUT` JSON. The parent and child both observe the actual current authority before allocating exports/staging or executing the candidate. This is test-only preparation: it does not enable public changed-schema admission or qualify any native crash phase by itself.

## Exact input and observation

`freshAuthority` denies unknown or missing fields. Required fields are `generation`, `head`, `scope`, the complete existing `Plan` object, `envelopeSha256`, `artifactSha256`, `catalogSha256`, `originalSelectionSha256`, `publicKey`, `managerSourceCommit`, `bindingSha256`, `hostArchiveSha256`, `trustArchiveSha256`, and `publicationPhase`. The full Plan retains operation/source/target/images/schemas/backup/inventory/signed-space requirements; no shorter competing Plan schema is introduced.

The new phase must be `pending-synced`, `selection-renamed`, or `selection-dir-synced`. Only the existing protected synthetic fixture namespace is eligible. Its consumed baseline generations through43 and the three historical operation IDs are refused even if a caller changes their generation in JSON. The issuer must first prepare a genuinely new signed operation under the current coherent authority; a terminal RolledBack record, copied receipt or an edited old input is not a prepared operation. An unknown future generation is accepted only when it exactly equals the valid current Prepared chain, head, scope, complete Plan and pinned policy key. Normal Manager policy/generation semantics are unchanged.

The qualifier checks a clean exact Manager Git commit; current accepted Ed25519 envelope, expiry, development/linux-arm64 target and complete plan binding; full actual signed artifact bytes; bounded migration catalog bytes; exact selected original and selection bytes; and the three exact retained checkpoint files. `Store::open_mode(..., false)` inspects the chain without writing interruption recovery. Its exclusive `trust.lock` is retained throughout the observation. Immediately before returning, the actual latest committed record is read again and its hash, generation, stage, scope, Plan and key are compared; selection, source commit/cleanliness and expiry are reobserved after the large file reads. A second Store cannot be opened while this same exclusive lock is retained.

These checks are prerequisites, not full recovered-data proof. The existing opaque execution permit still requires current consistent recovery, archive authentication, image/catalog/migration compatibility, actual candidate application/readiness and later cold recovery. A Git observation does not attest that an arbitrary binary was compiled from that commit.

## Unique phase resources and ordering

For each phase, use a separate newly signed operation. Create an empty private point named `native-publication-<phase>-<operationId>` immediately beside the original profile. Its separate checkpoint directory is named `checkpoint-<phase>-<operationId>` beside the same profile; binding, host and trust inputs must all belong to that checkpoint and match the supplied hashes. Original/retained files and baseline43 are preserved. The same operation reserved for another phase is refused; existing failed/successful point directories must not be deleted to make this check pass.

The parent first validates the empty point and exact authority, then performs the existing disk admission (all additional allocations plus signed headroom plus6GiB retained floor). Only after that does it create the two small budget reports and child output/error files. The child accepts only those four initial files in its point, independently rechecks all authority/selection/source/checkpoint constraints, and only then creates exports/staging/inactive candidates. A publication child invoked without `freshAuthority` refuses before execution; legacy whole-recovery qualifiers remain separate historical evidence.

If the point, checkpoint, signature/time, source, selected original, authority or capacity fails, stop and preserve inputs. No old operation is restarted and no original data, key, history, volume or backup is deleted. Native execution needs a newly admitted disk budget and explicit fresh fixture inputs, not a switch that turns caller-supplied success flags into proof.

## Verification and remaining gates

Focused Rust tests use the existing cached release target and offline dependencies:

```sh
cargo test --release -p exhibitos-lifecycle --lib fresh_publication_authority --locked --offline
```

They exercise exact Prepared binding, unknown/missing/duplicate raw fields, invalid Plan/phase, consumed generation/operation IDs, changed generation/head/scope/backup/key/source and dirty source. Small actual private files also verify exact streaming hash and preserved bytes with hardlink/symlink/quota refusal, shared mode/FIFO/directory early refusal, and replacement of a path after the first read from its original open file descriptor. Parent/child linkage is compiled, but these focused tests do not open the real retained native fixture, start Docker or execute migration/publication.

Actual fresh native `pending-synced` and `selection-dir-synced` executions, their exact cold selection reconciliation and original full-data recovery remain **NOT_RUN** until fresh signing/checkpoint preparation and sufficient disk capacity. The prior genuine `selection-renamed` result belongs to its recorded historical operation; this change neither reruns it nor transfers its PASS to another phase. Power-loss durability, native GUI/VoiceOver, the deferred Windows final batch and T08-02 full acceptance remain separate gates.
