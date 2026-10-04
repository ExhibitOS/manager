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
