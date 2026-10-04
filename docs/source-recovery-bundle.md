# 같은 fence의 현재 DB·작품·설정·이미지 관측

```sh
exhibitos-update verify-update-source-recovery-bundle --profile /absolute/private/profile --installation default --image sha256:trusted-local-maintenance-id --external-writers-quiesced --apps-closed
```

Prepared의 원본과 완료된 등록 recovery 후보를 기존 ExecutionSession에서 확인합니다. Store anchor/legacy session과 두 installation operation 잠금을 함께 보유한 상태에서 세 관측을 실행합니다: current DB/blob full inventory → complete configuration/image bytes → repeated current DB/blob full inventory. 기존 단일 관측 CLI의 동작은 유지하며 private scoped 경로만 이미 보유한 installation 잠금을 재취득하지 않습니다. Caller/network 입력으로 잠금 생략이나 성공 receipt를 제공할 수 없습니다.

원본 DB는 정지 상태로 두고 독립 native volume copies에서 PostgreSQL을 실행합니다. 원본 blobs/configuration은 read-only 관측합니다. 두 DB 관측이 같은 원본 physical census/full inventory/schema를 보존해야 하고 설정/image 관측도 동일 raw authenticated backup manifest/source/target에 묶여야 합니다. 원본/후보 경로 inode/registry와 deployment 파일/실제 Docker container/volume binding을 재검사합니다.

Receipt는 `dataInventoryVerified`, `configurationInventoryVerified`, `imageBytesVerified` 결과를 보고하지만 `preflightVerified`, `updateExecuted`는 false입니다. 정확한 현재 데이터가 이미 인증된 계획의 백업과 같음을 검사하며 새 백업을 만들거나 원본 DB를 시작하지 않습니다. Image export·독립 snapshot·실패 진단 사본은 보존하고 원래 데이터/backup/keys를 삭제하지 않습니다.

실제 macOS ARM64/Docker genuine Prepared fixture에서 DB/blob 앞뒤, 설정7개·이미지2개 전체 bytes, 반복 원본 census가 같은 인증 백업과 일치했습니다. 원래 deployment5파일/current trust/Prepared intent와 모든 기존 container states가 보존됐습니다. Missing acknowledgement와 없는 maintenance image는 거부했습니다. 합성 filesystem 검사도 missing candidate/acknowledgement의 Engine-before 거부를 확인합니다. 초기 regression test를 다른 fixture 모듈에 넣은 compile 실패는 올바른 모듈로 수정했습니다.

이 결과는 장래 Applying permit이 아니며 privileged 외부 writer를 격리하거나 관측 이후의 변경을 막는 증거가 아닙니다. Host/trust 묶음과 아직 하나의 실행 범위로 연결하지 않았습니다. Authority-loss/current security+host+service 전체 복원, 실제 target application/rollback·interruption/OS/nativeGUI/cold/full corpus는 별도 인수 조건입니다.
