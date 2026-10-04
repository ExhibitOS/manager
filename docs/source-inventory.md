# Current source DB/blob inventory (development CLI)

The current original database must not be started merely to check update safety.
This adapter derives its source and completed recovery candidate from the
Prepared plan under exclusive borrowed trust/profile/source/target fences.

```sh
./target/debug/exhibitos-update verify-update-source-inventory \
  --profile '<canonical private profile>' --installation default \
  --image 'sha256:<trusted local maintenance image ID>' \
  --external-writers-quiesced --apps-closed
```

It authenticates the exact candidate raw manifest/receipt to the planned
backup/inventory/schema/runtime and checks current host5 installation files and
stopped source service/container/volume identities. It creates a fresh native
[stopped database copy](source-database-snapshot.md); no caller-selected snapshot
or database root is accepted. Host free space is at least6GiB, conservatively
reserving two bounded2GiB copies and a2GiB floor; each copy checks native storage.

A dedicated network-none/read-only-root maintenance helper starts PostgreSQL18
only against that new writable copy, as UID/GID999. A private Unix socket is
used; TCP listening, autovacuum, archive mode and preload libraries are disabled,
and transactions default read-only. Original DB is never mounted by the reader.
Original owned blob volume and exact candidate manifest file are read-only.
The helper needs only DAC_OVERRIDE/CHOWN/SETUID/SETGID/KILL capabilities in its
private process namespace, to read protected blobs, initialize the private socket,
drop the PostgreSQL child identity and stop its own child. No host PID/network,
original credentials/key mounts, image pulls or persistent original service starts.

The trusted Platform maintenance CLI `check-source-inventory` performs two fresh
read-only full censuses and verifies original database system identifier/database
OID, supported schema/migrations, all public table/sequence values, object bytes,
references and issues against the authenticated manifest. The closed result must
match exact plan backup/manifest/inventory/schema and must not claim configuration,
preflight or update execution. The isolated PostgreSQL child must stop successfully
before returning. Reader never exposes raw rows, blob keys or private credentials.

After comparison, another fresh physical original copy must match the first
source content digest/counts. Host files and stopped original container/volume
bindings are rechecked before returning. Both copies/private plaintext survive;
the first copy legitimately changes during its own PostgreSQL startup/shutdown.
A successful owned stopped helper is removed without force/volume removal. Failed
or uncertain helpers/copies remain for private diagnosis. Initial supported layout
is PostgreSQL18/UID999 with existing local socket authentication permitting the
application database owner. Other configurations safely refuse and require
separately verified handling; no hidden credential generation or hba mutation.

Success has `dataInventoryVerified:true` and preserves the full Prepared intent
and active selection. Configuration/preflight/update execution remain false.
These are observed matching snapshots plus operator acknowledgement, not global
privileged-writer fencing or a durable permission to apply later. Full configuration,
physical image byte proof, current resources/compatibility, coherent latest security
history/floors/revocations/reserved IDs recovery, signed target application/migration/
health/rollback and original OS/GUI/full-corpus acceptance remain required.

Actual qualification (existing retained synthetic fixtures only):

```sh
node scripts/test-source-inventory.mjs \
  ./target/debug/exhibitos-update '<retained candidate fixture>' \
  '<actual inventory CLI JSON>' '<earlier actual physical copy CLI JSON>'
```

The companion test-isolated-source-inventory.py qualifies the component reader
against an explicitly retained synthetic copy. Component success alone is not
the fenced/current-source integration proof. All fixture reports/keys/data belong
outside Git and require separate consistent data backup; Git bundles exclude them.
