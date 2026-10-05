# Experimental isolated runtime qualification components

These modules exercise a real development runtime image against a disposable native PostgreSQL copy. They are internal qualification components, **not a CLI installation/update command, a preflight permit, or an image-only rollback proof**. They must not run against a live application database or writable original volume.

The controlled native test uses the current signed OCI-qualified linux/arm64 runtime and an independently qualified PostgreSQL18 maintenance image. The database helper owns a network-none namespace and a bounded tmpfs physical copy. The runtime joins only that helper's namespace, with no published ports and no Internet route. It receives read-only original blob/configuration/environment mounts, copies them into a fresh tmpfs namespace, then permanently drops to UID/GID1000 before importing the actual image's `startLocalRuntime` implementation.

The runtime executes its real migrations, Freeze configuration, API, local web proxy and background worker. Qualification requires API health, three readiness observations, a bounded web response using the existing Host contract, graceful runtime shutdown, exact blob/configuration content and mode preservation, and a post-runtime full logical database/blob inventory matched to the authenticated original backup. This is stronger than comparing embedded migrations alone. It still does not prove changed-schema migration, whole application feature compatibility, rollback execution, full recovery activation, or Windows support.

The first development probe has explicit limits: database copy256MiB, blob copy128MiB, configuration8MiB/512entries. The helper uses1GiB RAM with swap disabled; the runtime uses768MiB RAM with swap disabled. At least2304MiB Engine memory is required. The snapshot tmpfs capacity is1GiB and runtime tmpfs256MiB; exceeding a budget refuses the probe, never falls back to a persistent volume. The original task's complete workload/migration/native acceptance remains required.

`database.mjs` needs a host-injected `DATABASE_COPY` string containing the trusted bounded native copy implementation. `target.mjs` needs `copy.mjs` embedded before its body. These scripts are embedded into new controlled containers; do not load JavaScript from data volumes. The database helper mounts `/source`, `/blobs`, `/manifest.json` read-only and receives the authenticated manifest SHA. The runtime mounts `/source-blobs`, `/source-config`, `/runtime.env` read-only. Only `/snapshot`, `/tmp` and `/probe` are writable disposable memory locations. Authentication values remain inside the container and never enter result JSON.

A passing test retains private source/hash/result receipts, compares original/candidate Engine and deployment state and current trust before/after, and repeats native source/candidate inventory observations. Successful stopped owned helpers can then be removed by exact ID; no Docker image or volume is deleted. Failed helpers are stopped and retained for diagnosis. Host observations alone do not replace a borrowed exclusive Store/source/target execution fence; integration into that session remains the next implementation step.

Run the bounded copy safety tests from the Manager repository:

```sh
node --test scripts/runtime-probe/copy.test.mjs
```

The tests use fresh synthetic filesystem scopes and cover exact bytes/modes/original identity, existing destination preservation, symlink/hardlink/set-ID refusal, byte/entry/depth bounds and parent aliases. Windows filesystem execution is not qualified by these Unix owner tests.
