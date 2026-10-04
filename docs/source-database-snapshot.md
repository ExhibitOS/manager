# Stopped source database snapshot (development CLI)

An update cannot compare its current original database by starting the original
stopped volume: PostgreSQL recovery can write to it. This development adapter
materializes an independent native-volume copy while the borrowed exclusive
trust/profile/source/target fences remain held. It does not accept an externally
supplied snapshot/proof or arbitrary source volume.

```sh
./target/debug/exhibitos-update snapshot-update-source-database \
  --profile '<canonical private profile>' --installation default \
  --image 'sha256:<trusted local maintenance image ID>' \
  --external-writers-quiesced --apps-closed
```

The Prepared plan must name the registered source and a completed, registered
recovery candidate. Its authenticated raw manifest and receipt must match the
exact planned backup/inventory/schema/runtime. Current five host installation
files are checked against that manifest before/after copying; source service
container IDs and the owned DB/blob/config volume names are checked stopped
before/after. The original DB volume is derived from the installed source.
Host free space must be at least4GiB, conservatively reserving a2GiB copy plus2GiB
floor; native snapshot storage must have source bytes plus2GiB available.

A unique labelled local volume is retained for every attempt. The helper has no
network, a read-only root filesystem, source mounted read-only without volume
population, and only the new snapshot mount writable. It uses UID0 with only
DAC_OVERRIDE, CHOWN and FOWNER capabilities to read protected source files and
preserve owners/modes inside the new native volume. It cannot mount host paths,
modify the original volume, start the original service, or load images. Maintenance
images must already exist with the supplied immutable local ID.

The streaming copier accepts at most2GiB/200000 entries/64 levels. It rejects
symlinks, hardlinked regular files, sockets/devices/FIFOs, setuid/setgid, and the
unqualified Docker Desktop host sharing backend. Sticky directory modes are
preserved (the supported official PostgreSQL parent requires this). Source
file identity/length/timestamps and bytes are checked around reading, all source
metadata/hash entries are read again, and copied content/UID/GID/modes match.
Each copied file is synced. File times are not restored and directory fsync/
powerloss durability is not qualified. Private data buffers are zeroed.

Initial support is the native PostgreSQL18 `18/docker` layout, with PG_VERSION18,
nonempty pg_control and no postmaster.pid. The maintenance pg_controldata checks
the **copy** reports `shut down`; other states fail with the copy retained.
No PostgreSQL server is started. Other majors/layouts, tablespaces and larger
volumes require separately verified handling; they are refused, not certified.

Success reports `physicalDatabaseCopied:true`, source/snapshot volume identity,
file count/bytes/content digest/cleanShutdown, exact plan source/target/backup and
manifest hash. It leaves `dataInventoryVerified`, `preflightVerified` and
`updateExecuted` false. The full Prepared intent and selection are unchanged.
This is physical copy evidence, not the current logical DB/blob/configuration
inventory equality, global privileged-writer isolation, schema compatibility,
full security-history recovery or signed update/migration/health/rollback proof.
Snapshots are private plaintext, outside Git; successful and failed copies are
never automatically erased. A successful owned stopped helper is removed without
force/volume deletion; a failed or uncertain helper is retained for diagnosis.

Actual synthetic source proof and refusals:

```sh
node scripts/test-source-database.mjs \
  ./target/debug/exhibitos-update '<retained candidate fixture>' \
  '<actual snapshot CLI JSON>'
```

The last argument must be an actual result from the command above. The harness
checks the completed plan-bound source/copy, missing acknowledgement/failed
candidate/current deployment refusals, complete Prepared/Engine preservation,
and private synthetic hardlink/symlink/FIFO refusals before copying. It does not
replace the further logical database comparison or original task acceptance.
