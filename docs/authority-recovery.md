# Independent current authority recovery (Unix development)

A full host archive and a decrypted trust checkpoint do not prove which trust generation was last accepted after the live trust namespace is lost. Before an update, a trusted administrator can enroll an independent private authority vault while the complete original Store still exists:

```sh
exhibitos-update enroll-authority-recovery --profile /absolute/private/profile \
  --installation default --vault /absolute/private/recovery/authority-vault --apps-closed
```

The vault destination must be new, canonical, outside the profile and live authority namespace, in a private owner-only parent. Enrollment copies only the small complete trust journal, never the host, artifact, DB, blob, keys or images. It binds a fixed scope locator to the existing journal, preserves policy/floors/revocations/intent/reserved operation and instance identities, and advances the journal generation. Older clients reject the new recovery-binding field; upgrade Manager before reopening an enrolled profile. Release protocol1 is unchanged.

Every later Store commit first durably publishes its exact candidate record to the vault. It then durably publishes the primary record and finally writes a completion marker. Opening the Store requires exact complete primary/vault equality and the latest completion marker. Missing or replaced bindings, mismatched prefixes, unknown/pending records, incomplete dual writes, aliases, unsafe permissions and stale primary generations refuse. An interrupted enrollment also prevents continuing with an unbound Store. Incomplete dual writes remain unavailable until the explicit reconciliation below succeeds; normal opens never silently repair them.

If only the original authority namespace is absent, with the profile, fixed scope locator and independently retained complete vault intact, close apps/writers and use:

```sh
exhibitos-update restore-missing-authority --profile /absolute/private/profile \
  --installation default --apps-closed
```

Recovery selects the vault through the previously bound fixed locator. It accepts no caller-selected archive, vault, generation or head. It checks the entire record chain, transitions, key revocations, monotonic floors, and reserved IDs; holds the profile/session/all registered operation locks; copies exact current records into a fresh private stage; rechecks the vault and publishes without replacement. Existing or partially present authority roots refuse and remain untouched. Unenrolled scopes never bootstrap through this command. Successful recovery restores live authority only; it does not restore host/service data, start a runtime, authorize Applying or claim update completion.

The private vault stores trust journal metadata under owner-only permissions. Include it and the fixed locator in consistent non-Git backup plans; ordinary Git bundles and encrypted historical checkpoints cannot replace the independently retained latest witness. Provisioning a new Store in an enrolled missing scope is refused. This mechanism protects loss/rollback of the primary namespace while the vault remains current and intact. It does not resist the trusted OS owner rolling back or deleting both namespaces, a missing scope locator, or loss of the entire storage device. Those cases require separately protected current evidence and remain outside this qualified recovery path. There is no automatic deletion, vault rollover or reset of floors/IDs.

Remaining whole-project gates include coherent host/service activation, owned preflight/execution, changed migration, plan-bound health, full rollback, cold/crash recovery beyond this authority-only namespace, and native Windows qualification. Unit or CLI authority recovery alone does not satisfy those gates.


## Interrupted authority publication

Close apps and writers, then reconcile the original namespace against its enrolled vault:

```sh
exhibitos-update reconcile-authority --profile /absolute/private/profile \
  --installation default --apps-closed
```

This command takes the existing profile, all registered operation, primary trust, locator and vault locks. Every completed generation since enrollment must have exactly one matching completion marker. It permits at most one final, fully validated, durable vault candidate without a marker, with the primary either identical or exactly one record behind. It publishes that exact candidate without replacement and then completes its marker. An already complete matching journal is unchanged. A fully copied enrollment interrupted after fixed locator publication can resume without changing policy, intent or reserved IDs.

Reconciliation accepts no caller-selected vault, generation, head or replacement record. A stale primary behind an already completed vault is refused; missing primary must use authority restore only after the vault is complete. Divergent/missing chain entries, marker gaps, multiple unmarked candidates, unknown/partial pending files, unsafe permissions and busy/replaced locks refuse while preserving existing bytes. Partial/truncated staging files and enrollment interrupted before a complete fixed locator/copy still require explicit diagnostic recovery and are not automatically removed.

No runtime work is replayed or attested. If the reconciled journal contains an in-flight update, the next normal Store open appends its existing Interrupted recovery transition and requires full recovery; it never resumes Applying automatically. The reconciliation receipt always reports host/service restoration and update execution false. Only the small retained trust journal is used; no host, DB, image or archive copy is made.


## Owned preflight admission (Unix framework API)

`ExecutionSession::prepare_owned_update` reuses the existing authenticated host/trust ciphertext, registered restored candidate and staged signed artifact. It runs the complete inactive host/trust restoration, current source/candidate inventory and actual runtime checks under one profile/trust session, then retains the **same** source and target operation locks. Temporary image exports retire only after exact-byte checks. An independently enrolled current authority vault is required before any extraction or Engine work. Its entire current history is restored to a small inactive directory and checked against the retained primary and vault.

The returned `OwnedPreflight` has no public constructor, JSON deserialization or cloning. `begin` consumes it, rechecks current authority, checkpoint, full host, key, stopped service identities, free space and fresh signature/artifact, and durably writes `Applying` before an executor can mutate runtime state. The returned `StartedUpdate` retains the original session, artifact, operation locks, checkpoint/key borrows and inactive recovery proofs. Saved receipt JSON cannot recreate either permission. External writers must remain quiesced throughout this lifetime. Equal schema still does not authorize image-only rollback.

This additive Rust API is **not yet connected to the CLI or GUI executor**. A positive end-to-end admission on the current full development fixture, actual apply, changed migration, plan-bound health, coherent host/service activation and full rollback remain unqualified. Dropping after admission leaves an in-flight journal; existing restart handling requires recovery and never assumes execution success. Whole-task completion criteria are unchanged. Windows admission remains unavailable; Windows compilation is not native execution evidence. Existing diagnostic APIs retain their read-only behavior and returned shape.

For minimal storage, keep one verified original recovery corpus and authenticated ciphertext/key, reuse it for the next execution qualification, and retire only newly created successful plaintext/export duplicates after independent verification. Preserve failed/ambiguous scopes and original data. This API does not delete older checkpoints, backups, profiles, images or volumes.
