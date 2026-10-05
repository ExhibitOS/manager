# Genuine Runtime release plan and restore-candidate qualification

The candidate fixture harness can now prepare a new update plan using an actually
qualified Runtime archive instead of its legacy synthetic1024-byte placeholder.
This is development qualification, not production trust provisioning or execution.

```sh
node scripts/test-bound-candidate.mjs '<built update CLI>' '<built Manager CLI>' \
  '<retained synthetic backup creation fixture>' \
  --genuine-release '<private genuine artifact qualification workspace>' \
  --workspace-parent '<canonical operator-owned private0700 parent>'
```

The supplied workspace contains runtime.tar and oci-final-proof.json from actual
OCI graph/layer/migration qualification. Artifact length/hash must match and the
source schema must equal the authenticated backup's full schema/migration
fingerprint. The target image/schema/version come from that real proof. The tool
creates a new private fixture root, copies the actual artifact and generates an
independent ephemeral Ed25519 development key in process memory. A public fixture
policy is provisioned in new stores only; prior deployment policies/history are
never replaced. The Rust prepare-update command verifies signature, full actual
bytes and exact plan binding, and persists its new prepared intent and floors.

Separate new registered candidate IDs and operation IDs are reserved. The harness
checks an authenticated wrong-manifest refusal, then restores the actual synthetic
DB/blob/config/backup images into the exact registered fresh target. It verifies
original-version Runtime readiness, database witness, blob/freeze key preservation,
administrator login and web response; repeated candidate restoration refuses
without overwriting completion. Finally it stops the candidate while retaining
all volumes/plaintext/ciphertext, original selection/intent and source states.
A genuine-release-binding.json records the real artifact/image/schema and plan
without serializing the signing private key.

The restored candidate initially runs the **original backup Runtime image**.
This does not test the new target Runtime, migration compatibility, update health,
actual application/rollback, latest coherent security-state recovery or a complete
preflight. The intent stays prepared and preflight remains absent. Observed backup
inventory identity is not fresh current-source equality. Equal migration hashes
do not establish old-runtime read compatibility or image-only rollback safety.
Current durable trust/time/source/space/resource/compatibility and consistent
backup+security-state recovery must be recomputed under execution fences before
mutation. OS/native GUI/device/cold-engine/full corpus gates are unchanged.

Legacy synthetic mode and observation-only resume mode remain distinct. The
previous1024-byte plan is never mutated into a deployable one. Resume of a genuine
fixture is not supported by this option; preserve any interrupted candidate and
inspect its actual state before an explicit subsequent recovery operation.

### Candidate inventory without persistent snapshots

`verify-restored-candidate-inventory-ephemeral` uses the same registered recovery candidate, retained source/candidate locks, current authority, immutable restoration receipt and authenticated manifest checks as the persistent inventory command. It performs two fresh physical copies and two native PostgreSQL18 full database/blob inventory observations. Both physical proofs must match, each logical proof must match the exact plan, and final source/candidate/receipt/manifest guards must still hold. It supplies no preflight or update execution permit.

```sh
exhibitos-update verify-restored-candidate-inventory-ephemeral \
  --profile /absolute/private/profile --installation default \
  --image sha256:QUALIFIED_LOCAL_MAINTENANCE_IMAGE_ID \
  --external-writers-quiesced --apps-closed
```

Each helper has no network, a readonly root, readonly original database/blob/manifest mounts, restricted capabilities, a64-process limit, 3GiB memory and equal memory+swap limit. The snapshot is a4GiB tmpfs capacity with a3GiB total process/memory ceiling; the existing physical2GiB copy limit and2GiB free-space headroom remain. The Engine must report at least3.5GiB RAM. This is an eligibility check, not a reservation: other workloads or large copies may still cause OOM, which refuses verification. Host/VM paging is outside this helper's guarantee. Base-image declared volumes are masked by tmpfs; unexpected declared volumes are refused before create to prevent accidental anonymous volumes. macOS Docker Desktop's exact `/host_mnt` bind representation is accepted; unrelated paths and writable original mounts are refused. Windows runtime qualification remains pending.

Tmpfs disappears when the helper stops, including failures; no persistent failure DB copy is retained by this opt-in path. Original volumes and host records remain untouched. Failed helper metadata and small private command reports are retained for diagnosis. Only exact owned, stopped helpers whose physical/logical proof validated are removed automatically; this path contains no volume deletion. Persistent snapshot commands remain available when a durable diagnostic copy is required. Existing snapshots and previous recovery baselines are not retired by this command.
