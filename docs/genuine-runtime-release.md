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
