# Local release trust journal v1

The app-closed `exhibitos-update` development commands persist trusted policy,
revoked key IDs, accepted release sequence/issued-time floors and the last observed
OS time. They perform **no runtime update, activation, migration or backup restore**.
The existing read-only `verify --policy` command remains independent and never
claims persistent replay protection. Use the persisted store for release acceptance.

## Provision separately from a release feed

A trusted deployment administrator provisions a public-key policy through a private
local file. Do not obtain trust roots or policy instructions from an untrusted feed.
No production signing key/root is supplied. Policy file ownership/current UID,
no group/world write, regular-file identity, no symlinks/hardlinks, bounded reads and
closed JSON schemas follow the [signed-release verifier](signed-releases.md).

Close all Manager apps and CLI operations for the profile before these commands.
The CLI requires the explicit `--apps-closed` acknowledgement and also obtains the
stable profile pathname fence exclusively. Concurrent cooperative controllers and
recovery commands refuse; the acknowledgement alone is not evidence of quiescence.
Use the same absolute logical profile pathname and `default` or registered
installation UUID on every run. UUID syntax is checked; the development CLI does
not prove that a supplied UUID is registered or that a source schema matches the
actual database. Installation routing and executor binding remain required.

```sh
./target/release/exhibitos-update trust-provision \
  --profile '<absolute logical profile>' --installation default \
  --policy '<separately trusted policy.json>' --apps-closed
./target/release/exhibitos-update trust-status \
  --profile '<same profile>' --installation default --apps-closed
./target/release/exhibitos-update accept \
  --profile '<same profile>' --installation default \
  --release '<signed envelope.json>' --artifact '<exact signed basename>' \
  --apps-closed
```

`trust-provision` exclusively creates a new store and refuses an existing one.
`accept` verifies against its stored policy, reads/hashes the actual artifact,
rechecks file identity, policy/expiry/time and exact prior verifier proof, then
commits the new minimum sequence/time floor. Missing/bad signatures, artifact
failures, expiry or clock regression never advance the floor. Successful acceptance
consumes that sequence even if a later update fails; it is not installation success.
A future retry must use a durable operation record bound to this accepted payload,
not reset security floors or blindly accept the old envelope again. That retry and
mutation-time trust recheck are not yet connected to the update executor.

The receipt includes `verification` and `trust`; `activated:false` and
`backupRestoreVerified:false` remain explicit. A store status/policy receipt is not
an artifact verification receipt. Last observed time never decreases. OS clock
regression refuses; this does not authenticate real wall-clock time or protect
against an administrator setting the clock forward.

## Authorized policy replacement and revocation

```sh
./target/release/exhibitos-update trust-policy \
  --profile '<same profile>' --installation default \
  --policy '<administrator-provisioned replacement policy.json>' \
  --expected-generation '<policyGeneration from trust-status>' --apps-closed
```

The expected policy generation must match the current stored generation; stale
administrator instructions refuse. Channel, container target and protocol stay fixed; sequence/time floors cannot
lower. Removed key IDs remain revoked and cannot be reintroduced in this store.
The pinned source schema can change through an explicit trusted administrator
policy action, including a deliberate data rollback; this action is not a verified
migration. No policy is accepted from release metadata. Remote signed rotation,
root expiration, recovery authorization, a production publisher and deployed
roots remain unimplemented. The OS account administering this store is trusted;
these commands do not provide a separate server role boundary.

## Persistence, recovery and limits

A deterministic scope hash includes the canonical logical profile pathname and
installation identity. The private700 store is a sibling directory named
`.exhibitos-release-trust-<scope hash>`, **outside** replaceable/archived profile
bytes. Its private600 lock and exclusive profile anchor stay held throughout each
operation. Old profile restore, absence or replacement does not recreate/reset the
store. A missing or corrupt store refuses open/accept and never automatically
bootstraps from an archive or feed. Parent-folder relocation changes the namespace
and requires a separately authorized security-state migration; it is not automatic.

Each immutable, bounded96KiB record contains the policy, monotonically increasing
journal/policy generations, observed time, revoked IDs, acceptance identity and
previous record hash. On open, the full contiguous chain is checked, including
monotonic policy transitions and scope. New records use fresh private staging,
file fsync, OS atomic no-replace rename and directory fsync. Publication collisions
never overwrite existing records. Partial writes remain private `pending-UUID.json`
files for diagnosis, without advancing floors. A write/sync error reports
`UPDATE_TRUST_WRITE_UNCERTAIN`; that live Store refuses further verification/writes
and marks diagnostic receipts `writeUncertain:true`. Close and reopen before any retry, inspect the
persisted state, and never assume the previous floor still applies. No automatic
record, pending file or user-data deletion occurs.

Current bounds:4096 committed records (96KiB maximum per record),256 permanently revoked IDs, at most65
additional directory entries including the lock/pending records. Exceeding bounds
fails closed. Automatic compaction/rotation is absent and must preserve the highest
security floors and revocations. Record hashes detect chain corruption/gaps; they
are not a signature/MAC against the trusted OS account. Deleting the whole store,
removing its final suffix or replaying a complete old store can evade these local
checks. Hardware-backed/remote monotonic recovery protection is not implemented.
Do not restore an old security journal as part of a profile/data rollback.

These files are Git-excluded application security configuration. Existing profile
and host checkpoints deliberately exclude them. A complete installation recovery
must back them up consistently with its update operation records and preserve the
**highest** policy/floors/revocations against any older snapshot. Until a verified
security-state recovery protocol exists, retain the original external journal and
anchor during host recovery. Git bundle backups do not include this state.

macOS atomic publication and filesystem behavior are locally tested. Linux uses
`renameat2(RENAME_NOREPLACE)` but requires actual OS qualification; unsupported OS
paths, including Windows ACL/publication, refuse. Actual power-loss/disk-failure qualification remains required. No native app package/UI build or
production OS signing qualification is implied by CLI tests. Old binaries unaware
of this journal must not be used for release acceptance.

The [persisted update preparation](update-intent.md) binds actual signed bytes and
plan to the same atomic acceptance record; it does not execute an update.

## Renewing an unchanged Prepared release

`renew-prepared-update` accepts a new signed envelope only for the exact current
`Prepared` operation and expected trust generation. It verifies actual artifact
bytes and a fresh signature under the currently pinned policy. The full update
plan, candidate identity, artifact name/size/hash/image/schema, version,
channel/target/protocol and ordered source-schema list must remain unchanged.
Only a strictly higher sequence, nondecreasing issue time, later expiry and
trusted signing signature/key may change. Expired original metadata is not used
to authorize execution; the new envelope must independently pass current policy,
clock, expiry, signature and artifact validation.

The immutable `renew_prepared` journal event retains the existing operation and
candidate rather than discarding and creating another copied installation. Replay
reverifies the new signature at the committed time and the exact permitted
transition. All previous records, reserved IDs, revocations and monotonic floors
remain. Applying, recovery and completed operations cannot be renewed. Current
checkpoint and opaque artifact/runtime proofs become stale at the new generation
and must be requalified; this operation supplies no compatibility or preflight
permit. No Docker/container/volume/data mutation occurs.

```text
exhibitos-update renew-prepared-update --profile <absolute-private-profile> --installation default --release <new-signed-envelope> --artifact <unchanged-immutable-artifact> --operation-id <current-prepared-operation> --expected-generation <current-trust-generation> --apps-closed
```

Journal compatibility: this is an additive event in the closed version1 local
journal schema; signed release protocol1 is unchanged. A prior Manager that does
not recognize `renew_prepared` rejects this history and cannot open or execute it.
Use the updated Manager before publishing this event; do not erase records or
lower floors to downgrade. Native Windows journal publication remains a separate
qualification gate. Keys are never read from feed payloads or stored by this CLI.


## Release a completed operation before preparing the next update

An `Updated` or `RolledBack` operation keeps its terminal intent until an explicit
administrative release. Inspect `update-intent`, reconcile owned runtime/helpers
and preserve recovery candidates first. Then close all profile controllers:

```sh
exhibitos-update release-completed-update --profile /absolute/private/profile \
  --installation default --operation-id <current-operation-id> \
  --expected-generation <current-trust-generation> \
  --runtime-reconciled --preserve-data --apps-closed
```

This appends a trust record and clears only the active terminal pointer. It keeps
history, used operation/instance IDs, replay floors, host files, candidates, keys
and engine resources. Prepared, inflight or recovery-required operations, stale
generations, wrong operation IDs and missing acknowledgements are refused.
`--runtime-reconciled` is an operator acknowledgement, not evidence that this
command checked Docker or runtime health. The output explicitly reports
`runtimeReconciliationAttested: false`, `executed: false` and `runtimeChanged: false`.

After release, create a genuinely new operation/target identity and verify the
current source and a coherent current host/authority/service recovery checkpoint.
Do not replay the released intent or treat its historical checkpoint as proof
of the newer authority generation. Public changed-schema execution remains
closed until its full native recovery qualification is complete.


## Compare a complete host archive without a plaintext copy

After closing controllers and stopping host writers, use the manifest hash from
the retained checkpoint receipt:

```sh
exhibitos-profile --profile /absolute/private/profile verify-host-current \
  /absolute/private/key /absolute/private/host.bin <host-manifest-sha256> \
  --apps-closed --host-writers-stopped
```

This authenticates every archive frame through EOF and compares the complete
current file/directory inventory, bytes and modes under profile/runtime locks.
It creates zero plaintext extraction files, rejects changed source, wrong key,
wrong manifest or missing acknowledgements, and preserves the archive and source.
It does not restore host/trust/runtime, verify external service volumes, qualify
a new signed update or replace the separate actual restoration acceptance gate.

`verify-host-current` currently qualifies Unix host locking only. Windows and
other non-Unix hosts refuse with `HOST_PLATFORM_UNVERIFIED` before reading or
creating host paths; native qualification remains a separate gate.


### Reuse a completely current host archive

On Unix, `exhibitos-update checkpoint-existing-host --profile <absolute-profile> --installation default --key-file <external-key> --host-archive <retained-host.bin> --host-manifest <sha256> --destination <new-external-directory> --host-writers-stopped --apps-closed` authenticates the entire archive and matches current files, directories, bytes and modes under profile/root locks before and after creating a current trust archive. Only `trust.bin` and `pair-binding.bin` are published; the host remains at its retained path. Changed files, wrong key/manifest, unsafe paths or existing destination refuse publication.

This creates current identity with `sourcePlanBound=false`; it does not inherit any historical source observation, restore host or authority, activate a runtime, or qualify preflight. Current native source/candidate observations and a source-bound finalization remain required before update execution. Do not substitute this receipt for those gates.


### Register the current prepared target

`exhibitos-update register-planned-update-target --profile <absolute-profile> --installation default --preserve-active --apps-closed` creates the exact recovery namespace reserved by the current Prepared plan. It takes no caller ID/path, preserves source selection/history, and starts no runtime. Missing intent, non-Prepared stage, source mismatch, an existing candidate/name or unsafe namespace refuse. Repeating it refuses without adopting or overwriting the earlier candidate. The older `register-update-target` continues to allocate a new random namespace for workflows that register before preparing a plan.


## 실패한 업데이트에서 원래 서비스 전체 복원

```sh
exhibitos-update restore-original-rollback \
  --profile /absolute/private/profile --installation default \
  --maintenance-image sha256:qualified-maintenance-image \
  --key /absolute/private/key.bin --archive /absolute/private/original-backup \
  --port 50369 --fresh-candidate --external-writers-quiesced --apps-closed
```

앱과 외부 writer를 닫고 실제 검증된 이미지·키·원래 백업·미사용 포트를 지정합니다. 이 Unix 명령은 RecoveryRequired인 동일 계획에 새 비활성 복원 ID를 등록하고, 계획의 정확한 backup/image/schema/inventory를 인증해 원래 DB/blob/설정을 새 서비스로 복원합니다. 동일 Store 수명에서 실제 inventory·image·설정·health를 다시 관측한 뒤 원자적 선택과 RolledBack 이력을 완료합니다. caller가 candidate ID·경로·성공 receipt·health 플래그를 제공할 수 없습니다.

기존 원본과 실패 후보·immutable 이력은 덮어쓰지 않습니다. 실패하면 새 후보와 진단을 보존하며 자동 재실행하지 않습니다. 최소 공간과 새 후보의 모든 원래 복원 조건을 유지합니다. 성공은 별도 새 복원 서비스의 실제 준비 상태를 의미하고, 원래 호스트·authority 분실 복원이나 다른 게시 crash 경계까지 완료됐다는 뜻은 아닙니다. Windows 경계는 지원 검증 전 거부합니다. 공개 changed-schema update admission은 계속 닫혀 있습니다.
