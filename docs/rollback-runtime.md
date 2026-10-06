# Native restored Runtime rollback completion

`Store::activate_restored_rollback` reobserves an already restored, reserved recovery installation on Unix. It requires operator quiescence and the same independent authority, profile lock and source/changed-target/restored-candidate operation guards. It does not start Runtime or accept caller health receipts, candidate IDs, URLs or commands.

Update and rollback now share a qualified maintenance inventory observer. Runtime packages need not ship the maintenance CLI or its new storage exports. The observer uses a bounded1MiB tmpfs at the maintenance image’s implicit PostgreSQL volume path, so successful observation helpers create no anonymous database volume. Network ID/ownership labels, immutable helper image, read-only root/blob mount, unique helper identity and zero exit are checked; physical database identity is queried and must match a supplied retained candidate identity when applicable.

The adapter binds the exact authenticated restore workspace and original plan, compares five source configuration files, observes exact container images, ownership labels, three distinct native volumes, writer census and HTTP readiness. A qualified local maintenance image observes the actual candidate PostgreSQL system identifier and performs the existing two-snapshot migration-aware database/blob inventory comparison. Its input is the authenticated bounded manifest handle, blob mount is read-only and the helper runs without capabilities. It observes the freeze signing key bytes and native permissions separately. These checks occur before and after selection publication. Only the exact fresh receipt can advance the original authority to `RolledBack`; Runtime and health are never replayed by journal reconciliation.

Closing an authority while it awaits rollback health clears active restore/health proof. A new authenticated `InterruptedRestoration` event preserves only a diagnostic candidate receipt. The native adapter must reobserve that candidate before `ResumeRollbackHealth` can resume it. Legacy `Interrupted` events and records without the new optional field keep their previous serialization and replay semantics. Modified diagnostic bindings are refused. No public frontend endpoint accepts a resume receipt.

The local qualification reuses a retained synthetic restored installation, briefly starts only its Runtime, completes native reobservation and selected installation/authority continuity, stops its containers and checks normal reopen. It does not create another database/blob/image archive copy. Earlier failed-update journal creation is synthetic; this is not proof of a real failed target apply, changed migration rollback, whole-host cold recovery, crash recovery at every native stage, Windows behavior, GUI integration or signed production delivery. Those remain required acceptance work.

Unexpected Engine/reader failures preserve authority, prior selection and uncertain helpers for diagnosis. An error after publication requires explicit journal reconciliation; it never silently reruns Runtime or deletes original data, archives, keys, volumes or images.

## Operator CLI

`exhibitos-update complete-restored-rollback --profile <absolute-profile> --installation default --maintenance-image <qualified-sha256-image-id> --external-writers-quiesced --apps-closed`

This Unix command requires an existing reserved, restored candidate in the authenticated failed update intent. The maintenance image must be an existing locally qualified immutable `sha256:` ID. Both acknowledgements are required. Unknown/extra options, caller candidate IDs, health receipts and commands are rejected before opening the authority. Do not run it on the live development installation until complete reversible recovery qualification is established.

The command does not start the recovered Runtime. The operator must have already started only that qualified candidate and quiesced external writers. Its native image/schema/configuration/inventory and readiness checks determine completion; no success flag supplied by the caller is accepted. A terminal `RolledBack` operation refuses a second execution. If interrupted after selection publication, explicitly use `reconcile-selection-activation` for the exact operation before fresh health reobservation; metadata reconciliation does not replay Runtime or health.

Windows returns `UPDATE_TRUST_PLATFORM_UNVERIFIED`. Actual Windows/native GUI and end-to-end CLI positive qualification remain separate required gates. The CLI is connected to the actual native adapter, whose prior isolated Rust qualification does not itself prove CLI end-to-end execution.

## Current update migration boundary

The update application adapter currently refuses `source_schema != target_schema` with `UPDATE_RUNTIME_MIGRATION_UNQUALIFIED` before deployment publication. This is an implementation boundary, not a completed changed-migration recovery qualification or merely a device/permission wait. Adding changed-schema application requires explicit artifact-bound migration compatibility, verification of retained business/blob/configuration data against the original backup, actual failed migration and fresh full restore/cold/crash evidence. The existing refusal is retained until those conditions are implemented and exercised; it cannot be removed just to make an integration test pass.

The fixed historical target Runtime image also lacks `verifyRestoredInventory` and `scripts/service-backup.mjs`. Runtime API smoke checks did not exercise those previously missing update-health dependencies. The shared qualified observer addresses that execution bug; actual full update admission/application and migration recovery remain independently required.
