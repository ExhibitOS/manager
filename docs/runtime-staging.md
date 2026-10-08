# Signed Runtime staging development CLI

`exhibitos-update stage` copies authenticated artifact bytes to a new private
workspace before future bundle import. It never loads an image or changes an
installation, trust journal, source service, DB or volume.

```sh
exhibitos-update stage --policy '<separately trusted policy.json>' \
  --release '<signed envelope.json>' --artifact '<runtime.tar>' \
  --staging-parent '<existing absolute canonical private0700 directory>'
```

The existing signature verifier authenticates metadata first. Unix staging
requires an operator-owned canonical0700 parent and operator-owned regular source
with one link and no group/other permissions. O_NOFOLLOW/O_NONBLOCK refuse final
symlinks and special files; source basename/length must match signed metadata.
The current development boundary is at most2GiB and free space must include
signed size plus2GiB reserve. Windows staging refuses until ACL/identity semantics
are qualified; it does not silently copy under weaker rules.

A UUID-named0700 directory and exclusive0600 file are created. A bounded64KiB
stream refuses a length overflow before writing that chunk. Source open-handle
and path identities/length/timestamps/link/permissions/ownership are compared
before and after copying. File and directory metadata are synced, staged mode
becomes0400, and a read-only handle is retained. The verifier hashes that handle
and compares its identity to the staged path before and after. Parent identity
and permission observations are checked around staging. Failed/repeated checks
clear the previous artifact-verification flag. Failed partial and successful
copies are retained for explicit recovery; no automatic cleanup occurs.

The development CLI rechecks signature expiry and the supplied policy snapshot
at completion, then rehashes the retained staged handle. It returns staged:true,
ociInternalsVerified:false and activated:false. Paths in this private local
receipt must not be copied into public telemetry. The library's StagedArtifact
is not Deserialize and its handle is private; it exposes a local diagnostic path
and reverify against an actually verified release. This is not an import permit.
A future importer must retain/validate the same handle, recheck current durable
trust/revocation/clock/source/preflight under operation fences, and verify the OCI
contents against signed image/schema identity immediately before engine access.

0400/private directories and metadata observations do not exclude privileged
or same-user external writers. These checks are not durable global isolation,
production recovery, update application, migration, candidate health or rollback.
Parent/host crash consistency and Windows/native GUI need separate acceptance.

Validation commands:

```sh
cargo test --workspace --locked -- --test-threads=1
cargo clippy --workspace --all-targets --locked -- -D warnings
python3 scripts/test-staged-release.py '<built CLI>' '<genuine private artifact workspace>' '<signature fixture workspace>'
```

Tests cover multichunk copy, source preservation, private modes/independent inode,
expiry clearing, unsafe parent, links, same-size changed bytes and staged path
replacement. The CLI fixture harness additionally checks actual full Runtime
archive bytes and source/staged preservation, keeping all copies outside Git.
