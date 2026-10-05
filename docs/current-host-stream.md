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
