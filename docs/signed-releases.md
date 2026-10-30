# Signed runtime release verifier v1

The offline `exhibitos-update` development CLI authenticates a release envelope
against a separately provisioned trusted policy, then streams and hashes an actual
local artifact. It reports verification only. It does not download, import an OCI
bundle, change the runtime, migrate data, persist trust floors, activate a release
or claim backup restoration. The existing [update safety model](update-safety.md)
still requires independently verified compatibility, backup/restore, current source
inventory, disk space and rollback evidence before execution.

## Additive native artifact admission (format 2, macOS)

The macOS library also accepts an explicitly separate native payload declaration.
The existing format-1 CLI, OCI artifact and execution APIs retain their original
types and semantics. They do not accept a native admission result.

Native policy uses the same closed policy fields, with `format:2` and
`target:"macos-arm64"`. Its signed payload has the format-1 release fields plus
`kind:"native-payload"`, `format:2`, and the same exact policy target. The native
artifact has exactly `name`, `bytes`, `sha256`, `nativeManifestSha256`, and
`schemaSha256`; it has no `runtimeImageSha256`. The envelope remains format 1,
using the existing signature domain, pinned key policy, protocol, sequence/time
floors, expiry, size limits and strict parsing. Native policy never grants an OCI
permit, and format-1 policy cannot admit the native payload.

`Store::verify_native` reads the existing retained authority without creating or
changing it. `native::stage` reuses the existing private staged-file guards and
hash verification, retaining identity and the exact signed declaration.
`Store::admit_native_payload` freshly rechecks those bytes and independently
re-admits the selected `Payload`, requiring its exact manifest digest. The
existing Payload admission still checks its target, file inventory and executable
permissions, including official app admission where required; publisher flags
cannot bypass it. Native release target naming does not rename the existing
`darwin-arm64` payload manifest platform.

The opaque result has no public constructor, deserializer or conversion to
`VerifiedRelease`, `Preflight` or an OCI executor input. Rechecking binds the
authority scope/generation/policy/record identity, time, staged-file identity and
current payload bytes; a failure clears admission. These calls do not write trust
floors, a profile, a workspace manifest/environment or an update journal, and do
not launch or extract the artifact.

The signed `schemaSha256` is only the authenticated publisher declaration. The
receipt always reports `schemaObserved:false` and `executionPermitted:false`.
Actual source inventory must later bind to an authenticated native backup and a
current observation; target schema/inventory must be observed in a separate owned
candidate using that byte-verified target payload. Reuse the existing
`collectServiceInventory` and schema fingerprint rules (physical schema digest,
schema version and every ordered migration name/checksum). The 15→16 legacy
inspector is not that general observer. Until those adapters, the existing
plan/journal/backup gates and retained old-payload fallback are connected and
verified, no native update, manifest transition or image-only rollback is implied.

The native publication history schema is a closed, versioned binding for an
inactive-configuration operation. A source-bound begin operation stages an
authenticated beforeimage archive and commits a pending binding. A separate
guarded operation can publish that exact configuration to the inactive target;
authenticated offline repair restores only its bound generated beforeimages and
retains the inactive marker. Neither operation completes preflight, activates a
target, permits execution or implements a native upgrade.

An additive held publication result can recheck the exact published files,
their identities and closed absence boundaries under the retained source and
session fences. This is configuration observation only: it does not reobserve
the target database or certify full preflight. A serialized receipt cannot
recreate the in-process observation.

Existing records omit
the optional field and keep their existing serialization. Ordinary lifecycle
writers remain blocked for the bound target even after repair; releasing that
candidate requires a future explicit transition. A reader
built before this schema rejects a history once a
native record exists, including after its terminal record: do not rewrite history
or lower trust floors to make an older reader accept it. Retain the matching new
reader and a separately verified original recovery baseline before any
workflow writes such a record. An intervening authority or policy generation
invalidates the bound continuation. Resumption would require a separate explicit
renewal transition and fresh revalidation; neither is provided by this schema.

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
The read-only `verify --policy` command does **not** implement floor persistence.
A fresh caller-provided policy does not establish replay resistance for previous
accepted releases. The separate [local trust journal](release-trust.md) now persists
policy, permanent local key revocation and sequence/time floors through explicit
app-closed `accept`; it does not execute an update or provide remote signed rotation.
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

### Native source inventory observation

`InstallationController::capture_native_update_source` uses the existing native
backup creation/authentication and source-inventory check to retain an opaque
`AuthenticatedNativeBackup`. Serialized backup receipts or caller-supplied hashes
cannot create this handle. The encrypted archive manifest digest and authenticated
plaintext manifest digest are separate identities.

`Store::observe_native_source` and `recheck_native_source` use that handle and the
owned, quiesced native Session's fixed maintenance entry to re-observe the source
inventory, without Docker. They reuse the existing physical schema digest,
schema version and ordered migration checksums. The result binds the source
profile/installation, authenticated backup, source Payload and current signed
target admission/authority generation. Changed source files, backup bytes,
selected Payload or authority refuse reuse. The ordinary backup receipt format
and existing OCI release path are unchanged.

This is source observation only. It does not independently observe the target
schema, grant execution, change a workspace manifest pin, migrate data or perform
rollback. The target publisher declaration remains distinct from observation.
Independent target candidate observation and existing plan/journal/backup/retained
old-payload integration remain required before a native update transition.
