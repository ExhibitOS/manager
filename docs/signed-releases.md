# Signed runtime release verifier v1

The offline `exhibitos-update` development CLI authenticates a release envelope
against a separately provisioned trusted policy, then streams and hashes an actual
local artifact. It reports verification only. It does not download, import an OCI
bundle, change the runtime, migrate data, persist trust floors, activate a release
or claim backup restoration. The existing [update safety model](update-safety.md)
still requires independently verified compatibility, backup/restore, current source
inventory, disk space and rollback evidence before execution.

## Trust and release contract

Policy JSON has exactly `format:1`, `channel` (`development` or `stable`), `target`
(`linux-arm64` or `linux-amd64`, the container runtime architecture),
`protocolVersion:1`, `sourceSchemaSha256`, `minimumSequence`, `minimumIssuedAt`
and `publicKeys`. Public keys are 1–8 distinct lowercase hex encodings of raw
32-byte Ed25519 keys. Weak keys, unknown fields and duplicate fields refuse.
Policy must be provisioned outside the release feed; the feed cannot supply its
own trust key. The CLI checks Unix policy ownership/current UID and no group/world
write, and refuses symlinks and multiply linked inputs. Windows trusted policy
storage/ACL is not qualified. The policy is public configuration, not a signing
secret; only an authorized deployment administrator should change it.

The signed payload is a JSON object with these exact fields:

| Field | Meaning |
| --- | --- |
| `format`, `product` | `1`, `ExhibitOS/runtime` |
| `channel`, `target` | Exact pinned policy values |
| `version` | Three unsigned decimal components, optionally `-` prerelease identifiers; no build metadata; stable refuses prereleases |
| `sequence` | Positive release generation, strictly above the persisted floor |
| `issuedAt`, `expiresAt` | UTC Unix seconds; at most seven days validity; no future issue or expired acceptance |
| `protocolVersion` | `1` |
| `sourceSchemas` | 1–16 sorted unique lowercase SHA256 schema/migration fingerprints, including the pinned current source |
| `artifact` | Exact `name`, `bytes`, `sha256`, `runtimeImageSha256`, `schemaSha256` |

Artifact name is a simple ASCII basename without path separators/escape. Artifact
size is 1 byte–64GiB. All fingerprints are 64 lowercase hex characters. This
initial protocol deliberately uses explicit schema/migration fingerprints, not a
version comparison that might overlook data-only migration. A publisher-signed
compatibility declaration is not proof that a real migration or old-runtime read
has been tested. Runtime/image metadata must also be independently validated by
the trusted bundle importer and actual engine observation.

Envelope JSON has exactly `format:1`, `algorithm:"ed25519"`, `keyId`, `payload`
and `signature`. `keyId` is lowercase SHA256 of the trusted raw public key.
`payload` is the exact signed UTF8 JSON string, not a parsed/reserialized object.
Signature is 64 bytes encoded as 128 lowercase hex characters. Sign this byte
sequence using ordinary Ed25519:

```
UTF8("ExhibitOS-runtime-release-v1") || 00 || u64be(payload UTF8 byte length) || payload UTF8 bytes
```

This provides application domain separation without a cross-language canonical
JSON dependency. Reformatting payload bytes requires re-signing. Envelope is
bounded to32KiB and payload/policy to12KiB before parsing; closed schemas reject
duplicate/unknown fields. The verifier uses pinned `ed25519-dalek2.2.0` strict
verification and weak-key checks as documented by [upstream](https://docs.rs/ed25519-dalek/2.2.0/ed25519_dalek/struct.VerifyingKey.html).
Locked `curve25519-dalek4.1.3` is in the patched range of
[RUSTSEC-2024-0344](https://rustsec.org/advisories/RUSTSEC-2024-0344.html);
`ed25519-dalek2.2.0` is outside the pre2.0 signing-key issue in
[RUSTSEC-2022-0093](https://rustsec.org/advisories/RUSTSEC-2022-0093.html).
These checks are not a complete dependency/security audit.

## CLI and proof boundary

```sh
cargo build --release --locked -p exhibitos-lifecycle --bin exhibitos-update
./target/release/exhibitos-update verify \
  --policy '<trusted local policy.json>' \
  --release '<signed release envelope.json>' \
  --artifact '<local file with exact signed basename>'
```

CLI supplies current OS Unix time, verifies metadata, reads the same regular file
handle with bounded memory, checks length/SHA256, verifies file identity/change
metadata against the current path, and rechecks release expiry before reporting.
It never executes artifact bytes. A successful receipt has
`signatureVerified:true`, `artifactVerified:true`, `backupRestoreVerified:false`
and `activated:false`. Invalid metadata/artifacts return bounded `UPDATE_*`
codes without raw paths, key contents or OS errors. No input is deleted or repaired.

Rust `VerifiedRelease` cannot be deserialized from network data. It records actual
signature verification and only records artifact verification after a successful
stream hash; a failed repeat clears that proof. `bind_preflight(evidence, now)`
requires a still-valid release, verified artifact and exact target image/schema
plus the **pinned** source schema. It clears stale signature/artifact flags on a failed recheck and sets only those flags on success.
Other safety fields remain untouched, so the update core still refuses missing
compatibility, restoration, source and space evidence. Hold/reverify a stable file
handle during a future actual import; this offline receipt cannot prevent later
artifact replacement or prove bundle internal content/image identity.

Persist release generation and issued-time floors durably only after complete
verification, under the installation operation lock. Failed downloads/signatures
must not advance them. Recheck policy revocation/floors, time, source identity,
artifact handle, compatibility and verified restore evidence at mutation time.
The current CLI does **not** implement floor persistence. A fresh caller-provided
policy does not establish replay resistance for previous accepted releases.
Reliable clock and private durable policy store, authenticated key rotation/
revocation, deployment-specific development/production roots, authenticated
transport with bounded staging, bundle validation, pre-update verified backup,
separate candidate execution/health and reversible rollback remain integration
work. A feed cannot override policy, architecture or source schema. Production
trust roots/private signing keys are not supplied by this prototype.

## Local validation and notices

```sh
cargo test --release --locked -p exhibitos-lifecycle --lib -- --test-threads=1
cargo clippy --release --locked -p exhibitos-lifecycle --all-targets -- -D warnings
node scripts/test-signed-release.mjs '<built exhibitos-update>'
python3 scripts/vendor-signed-release-notices.py --cargo-home '<local Cargo home>'
```

Node/OpenSSL independently generates a transient test key, signs exact UTF8 bytes
and verifies a real2MiB synthetic artifact through Rust CLI. Wrong key/domain,
tamper, replay/expiry/future, architecture/channel/schema, closed schema/path,
changed artifact and unsafe policy/link failures are checked. Signing secret stays
in process memory; only public policy, signatures, synthetic data and reports are
retained. Synthetic artifact is deliberately **not** a deployable OCI bundle.
Unit tests also check preflight binding and prove missing backup/compatibility
remains refused. No OS signing account, real update/migration/restore, native GUI,
Windows or production release acceptance is implied.

The notice script follows the exact Cargo.lock Ed25519 dependency closure
(including optional and build dependencies), checks registry archive checksums and
copies only original top-level license/notice files to `licenses/signed-release`.
It never extracts sources or executes upstream code. Inventory includes each
original file's SHA256. Tauri resource configuration includes this directory;
packaged artifact byte verification must accompany the next native build. Existing
profile-crypto notices remain unchanged. New signatures/keys/archives never belong
in source unless they are explicitly synthetic public test fixtures.
