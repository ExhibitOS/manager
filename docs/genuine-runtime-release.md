# Genuine development Runtime artifact qualification

The local development scripts qualify a real tagless Docker OCI archive and then
exercise the existing Rust release verifier against its entire signed byte stream.
They are operator qualification tools, not the production bundle importer or an
update executor. Inputs and reports stay in a private workspace outside Git.

```sh
python3 scripts/verify-runtime-oci.py --archive '<private absolute runtime.tar>' \
  --image 'sha256:<Docker content ID>' --source-manifest '<authenticated restored manifest.json>' \
  --source-commit '<Platform revision>' --output '<new private proof.json>'
node scripts/test-genuine-release.mjs '<built exhibitos-update>' '<private workspace>' '<proof.json>'
```

The OCI checker hashes the whole archive and every blob, follows the bounded OCI
index/config/layer graph, permits exactly one linux/arm64 runtime, checks decoded
layer diff IDs, expected Platform source/license/command labels, and all embedded
migration files against the operator-supplied authenticated backup inventory.
It refuses linked/unsafe archives, unexpected members and graph references,
unsupported media, migration whiteouts/path replacements, revision mismatch and
Python optimized mode. Limits are 2GiB archive/decoded content, 4MiB metadata,
100,000 outer members and bounded descriptor depth/counts. Decoded layers are
held in memory up to that limit; this is not a low-memory production importer.
The backup manifest must already have been authenticated by the backup verifier;
this script does not authenticate that input. Equal migration fingerprints do not
prove actual compatibility or downgrade-safe data.

Tagless BuildKit exports may carry an empty config and unknown/unknown platform
for a recognized attestation manifest referring to the selected runtime. Its
in-toto/SLSA metadata is hash-checked and kept separate from executable images.
Empty attestation subjects do not authenticate source provenance. Platform labels
are observations, not an independent proof that a publisher built those sources.

The Node script generates an ephemeral Ed25519 private key only in memory and
writes a separate development fixture policy, never modifying deployed trust
roots. It signs the actual archive size/hash, OCI content ID, schema fingerprints
and development version. The existing Rust CLI must verify signature and actual
file bytes while reporting activated:false and backupRestoreVerified:false.
Tests refuse changed payload, foreign architecture/schema, explicit replay floor
and a same-length one-byte modified private archive copy, then check original
artifact/policy/envelope preservation. All fixtures are retained. No secret key
is written to disk, Git, reports or a service.

This qualification does not load/tag/pull/start the image, apply migrations,
check candidate health, activate an update or perform rollback. Durable trust
floor/revocation and coherent current security/data recovery require separate
integration evidence. The prior synthetic placeholder release cannot be upgraded
into a deployable plan merely by this receipt; a new real-artifact plan must be
verified and bound to fresh source/candidate observations before mutation.
Windows/amd64, production signing authority and native GUI remain separate gates.


## Migration identity extension

The default inspector continues to require exact original embedded SQL equality.
For a strictly extended list, all three optional inputs are required together:

```sh
python3 scripts/verify-runtime-oci.py --archive '<private absolute runtime.tar>' \
  --image 'sha256:<OCI content ID>' --source-manifest '<authenticated original manifest.json>' \
  --source-commit '<observed Platform revision>' \
  --target-inventory '<private independently observed target inventory.json>' \
  --target-inventory-sha256 '<trusted exact target inventory bytes hash>' \
  --target-schema-sha256 '<signed expected target schema hash>' \
  --output '<new private proof.json>'
```

Both JSON inputs must be canonical private regular files, owned by the reader,
without aliases or hardlinks, at most16MiB, with duplicate JSON keys refused.
The target inventory pin binds its exact bytes; the target schema hash must match
its catalog digest, schema version and migration list. Every original SQL name
and checksum must remain the exact sorted prefix. The complete final-layer
embedded SQL list must equal the target list. Migration symlinks, hardlinks,
special files, non-SQL entries, whiteouts and duplicate protected paths within a
layer refuse, including a later symlink replacing an earlier regular SQL file.

The report separates `sourceSchemaSha256` and `targetSchemaSha256` and includes
`migrationMode`, `sourceManifestSha256`, `targetInventorySha256`, and
`targetMigrationsSha256`. Existing fields remain compatible. This is an optional
operator artifact-identity prerequisite; Rust import/application behavior is
unchanged and still refuses unqualified changed schemas. A caller-selected hash
is not catalog observation, artifact signing, migration compatibility, data
preservation or recoverability. Obtain the target catalog from actual independent
native qualification and bind it to the release and plan before any application.
Arbitrary data transformations require their own compatibility and full recovery
proof. The inspector never loads, starts, extracts or migrates an image.

Run `python3 -m unittest discover -s scripts -p test_runtime_oci.py -v` for small
real OCI tar graph and input-refusal regression tests. Temporary synthetic archives
are removed by the test fixture; original archives and data are untouched.

The Rust embedded adapter passes its already authenticated retained manifest bytes
directly to the inspector; it does not reopen a manifest pathname. Its exact
compiled-in ENTRY text is exercised in an isolated Python child with both archive
and manifest descriptors retained, including refusal of a mismatched manifest
pin. The outer Rust execution/session and changed-schema admission gates still
require their own qualification.

## Standard OCI exports

Tagless standard OCI exports may omit Docker `manifest.json`. The inspector
continues to verify the complete OCI descriptor graph, every blob, layer diff ID,
configuration, architecture, source label, command and SQL identity. If Docker
compatibility metadata is present, its exact config/layers and absence of tags
remain mandatory. Only a valid bounded UTC creation timestamp is accepted on a
standard executable descriptor; it is metadata, not publisher provenance. Ref-name
annotations, malformed dates, tags and unrelated descriptor annotations refuse.

## Fenced diagnostic catalog binding

`qualify_prepared_oci_with_catalog` and the development CLI
`qualify-prepared-oci-catalog` bind a private catalog input under the existing
execution/profile/source/target fences. The input must have only schemaVersion,
schemaDigest and strictly ordered name/sha256 migration rows, at most64KiB. Its
canonical schema hash must equal the prepared artifact's signed plan target.
Exact private input bytes, file identity/permissions/owner/link count, canonical
path and private parent identity are checked before and after the child. The child
receives retained bytes in memory and an explicit pin, never reopens the catalog.

```sh
exhibitos-update qualify-prepared-oci-catalog \
  --profile <private-canonical-profile> --installation <UUID> \
  --artifact <verified-runtime.tar> --staging-parent <private-parent> \
  --python <canonical-Python3.11-or-later> --source-commit <Platform-revision> \
  --target-catalog <private-canonical-catalog.json> \
  --target-catalog-sha256 <exact-catalog-bytes-pin> --apps-closed
```

This command does not import an image or start a service. A staged artifact copy
requires storage budget. Obtain the catalog from independently verified complete
restored context; a caller-supplied pin alone is not observation. Receipts explicitly
set targetCatalogObserved, runtimeCompatibilityQualified, preflightVerified and
updateExecuted false. No RuntimeCompatibility/apply/rollback permit is created.
The native changed-schema gate remains closed until a current opaque observation
and full failed-update recovery prove the required original scope.

## Native changed-schema runtime observation

`with_migrated_runtime_compatibility` binds a retained private catalog to the signed
prepared artifact and actual cached image layers. Under the existing exclusive
profile/source/candidate operation fences it copies the complete candidate database
into bounded tmpfs, reads original blobs/configuration readonly, verifies original
inventory with the exact embedded original SQL prefix, starts/closes the actual new
runtime, and verifies target SQL/catalog and all original rows/sequences/history and
blobs. Configuration and both original physical source/candidate observations must
remain exact. The database helper shares only its private loopback namespace with
the immutable target helper; no published ports or writable original mounts exist.

The returned opaque reference is borrowed only inside the fenced callback. Saved
JSON cannot construct it. The catalog file is rechecked after the whole observation.
Actual changed-schema compatibility observation does not itself grant admission,
application, selected-host activation or image-only rollback. Existing full host/
trust recovery and failure restoration gates remain required; the current executor
still refuses changed-schema application until that full integration is qualified.
