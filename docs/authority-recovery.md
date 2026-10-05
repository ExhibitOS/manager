# Independent current authority recovery (Unix development)

A full host archive and a decrypted trust checkpoint do not prove which trust generation was last accepted after the live trust namespace is lost. Before an update, a trusted administrator can enroll an independent private authority vault while the complete original Store still exists:

```sh
exhibitos-update enroll-authority-recovery --profile /absolute/private/profile \
  --installation default --vault /absolute/private/recovery/authority-vault --apps-closed
```

The vault destination must be new, canonical, outside the profile and live authority namespace, in a private owner-only parent. Enrollment copies only the small complete trust journal, never the host, artifact, DB, blob, keys or images. It binds a fixed scope locator to the existing journal, preserves policy/floors/revocations/intent/reserved operation and instance identities, and advances the journal generation. Older clients reject the new recovery-binding field; upgrade Manager before reopening an enrolled profile. Release protocol1 is unchanged.

Every later Store commit first durably publishes its exact candidate record to the vault. It then durably publishes the primary record and finally writes a completion marker. Opening the Store requires exact complete primary/vault equality and the latest completion marker. Missing or replaced bindings, mismatched prefixes, unknown/pending records, incomplete dual writes, aliases, unsafe permissions and stale primary generations refuse. An interrupted enrollment also prevents continuing with an unbound Store. Ambiguous interrupted dual writes remain preserved for explicit reconciliation; automated reconciliation is not yet implemented.

If only the original authority namespace is absent, with the profile, fixed scope locator and independently retained complete vault intact, close apps/writers and use:

```sh
exhibitos-update restore-missing-authority --profile /absolute/private/profile \
  --installation default --apps-closed
```

Recovery selects the vault through the previously bound fixed locator. It accepts no caller-selected archive, vault, generation or head. It checks the entire record chain, transitions, key revocations, monotonic floors, and reserved IDs; holds the profile/session/all registered operation locks; copies exact current records into a fresh private stage; rechecks the vault and publishes without replacement. Existing or partially present authority roots refuse and remain untouched. Unenrolled scopes never bootstrap through this command. Successful recovery restores live authority only; it does not restore host/service data, start a runtime, authorize Applying or claim update completion.

The private vault stores trust journal metadata under owner-only permissions. Include it and the fixed locator in consistent non-Git backup plans; ordinary Git bundles and encrypted historical checkpoints cannot replace the independently retained latest witness. Provisioning a new Store in an enrolled missing scope is refused. This mechanism protects loss/rollback of the primary namespace while the vault remains current and intact. It does not resist the trusted OS owner rolling back or deleting both namespaces, a missing scope locator, or loss of the entire storage device. Those cases require separately protected current evidence and remain outside this qualified recovery path. There is no automatic deletion, vault rollover or reset of floors/IDs.

Remaining whole-project gates include coherent host/service activation, owned preflight/execution, changed migration, plan-bound health, full rollback, cold/crash recovery beyond this authority-only namespace, and native Windows qualification. Unit or CLI authority recovery alone does not satisfy those gates.
