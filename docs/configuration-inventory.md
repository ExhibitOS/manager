# Complete supported current configuration and image bytes

The current configuration proof must cover every configuration declared by the
Manager-generated authenticated backup, including the freshly generated image
inventory. Historical extracted bytes or current image reference metadata alone
cannot supply that complete proof.

```sh
./target/debug/exhibitos-update verify-update-configuration-inventory \
  --profile '<canonical private profile>' --installation default \
  --image 'sha256:<trusted local maintenance image ID>' \
  --external-writers-quiesced --apps-closed
```

Under the borrowed exclusive trust/profile/source/target fences, the adapter
requires the exact Prepared source and registered completed recovery candidate.
Raw authenticated manifest/receipt binds backup/inventory/schema/runtime to the
plan. The configuration scope must be exactly the five Manager host files,
freeze-signing-key.json and manager-image-inventory.json; missing, duplicate or
additional configuration refuses instead of silently certifying a subset.

Current five canonical private host files match declared lengths/hashes before
and after. The native owned configuration volume's sole private freeze key is
observed twice with UID1000/0600/single-link/native-filesystem checks. Both current
Engine references and installed platform/database containers match the bound
image inventory before/after.

For each actual current immutable image content ID, a fresh `docker image save`
stdout stream is written into a new private0700 workspace/0600 artifact, with
exact authenticated archive byte budget checked before every write. Oversized
input is refused without writing past that budget. A reader timeout kills/waits
for the known local Docker client; partial artifacts remain. Successful files
sync and undergo a separate stable file hash check. Each exact export length/hash
must match its authenticated deployment archive record. Total budget is at most
2GiB; host free space must exceed declared total plus a2GiB floor. No image
pull/load/tag/remove or original container restart occurs.

The Manager-generated compact image inventory (original references, newly
observed content IDs, archive paths, actual exported lengths/hashes) must itself
match authenticated configuration bytes/hash. Thus all seven configuration files
are evidenced. Current Docker save byte serialization must match the archived
version; semantically equal but byte-different exports refuse safely and require
separately verified handling. This supported qualification does not infer other
Docker/Podman/native OS serialization compatibility.

Success reports seven named file proofs and two image archives with
configurationInventoryVerified/imageBytesVerified true. Data inventory, full
preflight and update execution remain false; complete Prepared intent/selection
are unchanged. These are observed bytes and metadata, not global privileged-writer
isolation, a durable later apply permit, target schema/resource compatibility,
coherent latest security history/floors/revocations/reserved IDs+host/external-volume
recovery or signed apply/migration/health/rollback and GUI/Windows/device acceptance.
Private current image artifacts/partial files remain outside Git and Git backups;
no previous data/archives/keys/backups are deleted.

Actual retained synthetic full-result/negative/preservation qualification:

```sh
node scripts/test-full-configuration.mjs \
  ./target/debug/exhibitos-update '<retained candidate fixture>' \
  '<actual configuration CLI JSON>' '<earlier actual data inventory CLI JSON>'
```

The harness independently hashes exported private archives, validates the exact
seven-file result, unchanged full Prepared, missing acknowledgement/failed target,
raw authenticated-manifest and current-env refusals, then verifies restored bytes
and persistent original containers/volumes. It is not target application evidence.
