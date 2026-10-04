# Development tagless OCI cache import

The operator tool imports a verified development Runtime artifact into the local
Docker image cache. It never installs or starts a container, changes volumes or
activates an update. Existing installation/trust-operation integration remains
required before this can become an application adapter.

```sh
python3 scripts/import-runtime-oci.py --cli '<trusted built exhibitos-update>' \
  --docker '<local Docker executable>' --policy '<trusted development policy>' \
  --release '<signed envelope>' --archive '<private0400 staged runtime.tar>' \
  --source-manifest '<previously authenticated restored manifest>' \
  --source-commit '<Platform revision>' --output-parent '<private canonical0700 directory>' \
  --development-cache-import
```

The tool opens the staged regular single-link owner file with O_NOFOLLOW and
holds that descriptor across graph validation, Docker stdin import and post-import
hash/identity verification. Rust verifies the signed release and full archive
through the current path before validation, immediately before import and after
completion; policy/envelope bytes and file identity must remain the original
snapshot. This new handle is independently revalidated; it is not the handle from
a previous separate Rust stage CLI process.

The OCI validator checks the complete bounded graph/layers/migrations and matches
artifact bytes, signed root OCI image/schema identity and version. Docker's
compatibility manifest must reference exactly the same runtime config and ordered
layers; no tags are permitted. Tag-producing OCI annotations and external URLs
are refused. BuildKit's empty config inline `e30=` is accepted only with its
recognized empty media type and exact two-byte `{}` blob. Attestations remain
metadata without an authenticated source provenance claim.

Only the explicit development channel/linux-arm64 path is accepted. The source
manifest must already have been backup-authenticated; this tool does not perform
that authentication. Caller-selected CLI/Docker binaries are trusted local tools,
not supplied by the release feed. Python optimized mode refuses. Commands have a
300s timeout and1MiB output limit. A failed/killed Docker client can leave daemon
import progress uncertain; preserve the cache/artifacts and inspect before retry.
There is no automatic image deletion or cleanup.

Before/after observations require exact persistent container IDs/images/states/
mounts, volume names and tag mappings. All prior image IDs must remain and only
the signed root image may be added. Post-import Engine identity, architecture,
source revision and actual RootFS layer diffIDs must match. Reports distinguish
an already-cached image from new cache content; this is not cold-engine proof.
Reports and malformed/partial files stay in a unique private workspace outsideGit.

This is a development cache operation, not a complete update executor. Current
persisted floors/revocations, installation fences, privileged-writer isolation,
coherent security/data recovery, compatibility/resources, candidate health,
activation/migration and rollback remain required.0400 and identity observations
do not prevent same-user/privileged concurrent modification. Windows/native GUI
and production publisher trust also remain separate gates.

The refusal harness uses a fake Docker sentinel and tests wrong revision,
same-size tampering, and an independently validly signed archive whose Docker
compatibility config points outside the qualified graph. Cryptographic acceptance
must pass for the last fixture while OCI validation must refuse before Docker.
All private keys exist only in the Node process memory.
