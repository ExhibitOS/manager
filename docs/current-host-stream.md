# 암호화 호스트 사본의 현재 내용 대조

`verify-combined-recovery-ephemeral`은 동일한 exclusive profile/trust 및 원본·복원 후보 operation fence에서, 기존 암호화 host/trust 복구점과 현재 원본·후보를 대조하는 개발 검사입니다.

```sh
exhibitos-update verify-combined-recovery-ephemeral \
  --profile /absolute/private/profile --installation default \
  --image sha256:qualified-maintenance-image \
  --export-parent /absolute/private/exports \
  --host-archive /absolute/private/pair/host.bin \
  --trust-archive /absolute/private/pair/trust.bin \
  --key /absolute/private/key.bin \
  --pair-binding /absolute/private/pair/pair-binding.bin \
  --external-writers-quiesced --apps-closed
```

경로·이미지는 실제 검증된 자료로 바꾸고 앱과 외부 writer를 정지한 뒤 실행합니다. 키는 채팅·Git·출력에 넣지 않습니다. 기존 backup·완료된 복원 후보·Prepared 계획과 current authority가 필요한 검사이며, 임의 경로의 저장된 성공 JSON으로 조건을 충족시킬 수 없습니다.

호스트 대조는 암호화 스트림을 읽기만 합니다. 최대8MiB manifest와 제한된 인증 frame을 메모리에 두고, 모든 file payload hash와 현재 파일 목록·바이트·권한·registry·허용된 lock 제외 집합을 검사합니다. 파일을 추가하거나 누락한 상태, 잘린 stream과 최종 인증 뒤의 trailing bytes는 거부합니다. 원본→후보→원본 서비스 관측 앞뒤에 호스트 전체를 다시 대조하고, archive identity/current authority도 다시 검사합니다. 성공 결과의 `host.currentProfileMatched=true`, `host.plaintextFilesCreated=0`은 이 읽기 검사에 해당합니다.

DB/blob 검사는 tmpfs 임시 사본에서 native PostgreSQL을 사용하고 디스크 volume을 새로 만들지 않습니다. 세 image 관측은 성공한 전체 fence 검증 뒤 새 고유 export만 정리하고 작은 hash marker를 남깁니다. 디스크14GiB eligibility와 기존 copy/image/memory 제한은 유지합니다. 실패 자료는 보존하며 기존 archive/key/data/volume/image를 삭제하지 않습니다.

이 읽기 검사는 실제 추출·authority 복원·복구 후 실행을 대체하지 않습니다. `sourcePlanBound=false` 복구점은 해당 한계를 그대로 표시합니다. 결과는 `preflightVerified=false`, `updateExecuted=false`이며 Applying 허가나 전체 coherent 복원 완료를 뜻하지 않습니다. Windows host archive 경계는 아직 지원 검증이 끝나지 않았습니다.

## 관측된 원본·계획에 연결하기

위 검사의 명령 이름을 `bind-observed-recovery-pair`로 바꾸고, `--external-writers-quiesced` 앞에 `--bound-pair /absolute/private/new-source-binding.bin`을 추가하면 새 인증 연결 파일을 만듭니다. 부모는 기존 private 디렉터리여야 하고 출력은 원본 profile 밖의 새 경로여야 합니다. 기존 파일·symlink·동일 이름은 거부하며 원래 binding을 덮어쓰지 않습니다.

전체 호스트 전후 대조·원본/후보 DB/blob·설정/키/이미지·마지막 공통 상태 확인에 성공한 뒤, 원본·후보 operation 잠금과 profile/trust fence를 계속 보유한 채 출력합니다. 현재 authority generation/head 및 정확한 operation/source/target/backup/manifest/inventory/schema를 AES-GCM 연결 파일에 인증하며, 기존 host/trust ciphertext 바이트를 복제하지 않습니다. 새 파일을 읽어 인증·전체 ciphertext/current authority를 다시 확인한 뒤 성공 결과를 제공합니다. 실패 자료는 보존하고 자동 재실행이나 덮어쓰기를 하지 않습니다.

결과의 `boundPair`는 새 파일 경로이고 `checkpoint.sourcePlanBound=true`는 관측한 현재 계획에 대한 provenance 연결입니다. 원래 catalog는 그대로 유지합니다. `verify-current-checkpoint-pair`에 새 파일을 지정하면 동일한 ciphertext와 현재 authority를 기준으로 읽기 재검사를 수행할 수 있습니다. 키나 operation/source/backup/current authority가 다르면 거부합니다.

이 연결은 새 릴리스 서명·만료·호환성 검사나 실제 복원 실행을 대신하지 않습니다. 기존 Prepared 계획의 provenance를 연결해도 만료된 릴리스를 적용할 수 없습니다. coherent authority/host/외부 서비스 복원, 복원 후 health, owned preflight/executor와 apply/rollback 검증은 계속 별도로 필요합니다. `preflightVerified`와 `updateExecuted`는 여전히 false입니다.

## 최신 신뢰 기록과 이전 복구점

Rust의 `Store::verify_rollback_checkpoint_pair`와 읽기 전용 `CheckpointVerifier::verify_rollback`은 복구 중인 동일 계획의 이전 Prepared 복구점을 별도 opaque proof로 확인합니다. 기존 `verify_checkpoint_pair`의 현재 generation/head 일치 조건은 유지합니다. 과거 복구점을 현재 복구점으로 취급하지 않습니다.

인증된 catalog의 generation/head가 독립 보존된 전체 신뢰 이력의 정확한 prefix여야 합니다. 당시 단계는 Prepared이고, 현재 단계는 RecoveryRequired·Restoring·AwaitingRollbackHealth 중 하나여야 합니다. operation/source/target/backup/manifest/inventory/schema와 전체 계획이 같아야 하며, 중간 이력에 다른 계획이나 삭제된 intent가 있으면 거부합니다. catalog·두 archive의 실제 전체 bytes를 읽어 확인하며 추가 사본을 만들지 않습니다.

결과에는 이전 checkpoint와 최신 retained generation/head를 구분합니다. 실행 직전 `recheck_rollback_checkpoint_pair` 또는 `recheck_rollback`이 필요하며, 그 사이 최신 정책·revocation·journal이 변하면 기존 proof는 거부됩니다. 최신 sequence floor·폐기된 키·예약된 ID·전체 신뢰 이력은 변경하지 않습니다.

이 API는 이전 host/trust 파일의 provenance 확인만 제공합니다. 과거 trust를 live authority로 활성화하거나 host·DB/blob를 복원하지 않습니다. 전체 복원·cold recovery·authority 분실·실제 apply/health/rollback·Windows native 검증은 별도로 필요합니다. 작은 합성 파일 검사 결과를 기존 전체 corpus 완료로 계산하지 않습니다.
