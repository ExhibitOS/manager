# 같은 잠금 범위의 host·trust 사본

```sh
exhibitos-update checkpoint-host-trust --profile /absolute/private/profile --installation default --key-file /absolute/private/key.bin --destination /absolute/private/recovery/new-pair --host-writers-stopped --apps-closed
```

현재 Store의 exclusive pathname anchor를 복제하여 기존 host checkpoint 경로에 전달합니다. 다시 anchor를 잠그거나 downgrade/unlock하지 않습니다. Legacy profile-session, profile operation, 모든 등록된 available root의 operation 잠금은 host inventory/암호화와 trust archive, host 재관측을 모두 포함한 범위에서 유지합니다. 일반 `checkpoint-host` 경로는 기존 잠금/형식을 유지하며 같은 내부 구현을 사용합니다.

호스트와 전체 trust 이력의 암호화 사본을 새 고유 private pending 디렉터리에 만듭니다. 키의 current bytes를 양쪽 생성 전후 대조합니다. 두 사본이 성공하면0600 `verified.json`에 receipt를 저장·fsync하고 새 `--destination`으로 no-replace 게시합니다. 원본 데이터를 교체하거나 writer를 재시작하지 않습니다. 기존 목적지·미확인 host writer 조건·공개/alias key·잠금 실패는 거부하고 실패한 staging을 보존합니다.

묶음의 `host.bin`은 기존 host checkpoint format이고 `trust.bin`은 trust checkpoint format입니다. 각각 기존 `extract-host`/`extract-trust-checkpoint`로 독립된 비활성 경로에 추출·검증합니다. Marker는 private 로컬 결과 기록이며 untrusted input을 실행 권한으로 승격하는 증명서가 아닙니다. 향후 묶음 소비자는 암호문과 각 내부 인증 데이터를 실제로 검사해야 합니다.

`externalVolumesSaved:false`, `liveAuthorityRestored:false`를 유지합니다. 호스트 안의 과거 dump/artifact는 보관되지만 현재 외부 DB/blob/config native volume을 새로 백업했다고 해석하지 않습니다. 전체 서비스 데이터와 일관된 최신 recovery point 통합, privileged writer 격리, authority-loss 복원/활성화·Applying 중단·Windows/Podman/GUI/full corpus는 후속 조건입니다. `--host-writers-stopped`는 운영자 확인입니다.

합성 filesystem 검사에서 Store anchor 보유 중 controller가 거부되고 borrowed-anchor 경로는 중첩 잠금 없이 성공하는 것을 확인했습니다. Prepared/trust head, 원본 파일, 기존 목적지와 미확인 writer 조건도 검사합니다. 테스트 fixture의 controller API 명칭을 잘못 사용한 초기 compile 실패는 수정했으며 인수 기준은 변경하지 않았습니다. 전체 직렬 lifecycle172unit+14integration=186PASS,4Engine skipped 및 strictClippy/releasebuild PASS.

실제 genuine generation2 Prepared 프로필의 전체 host bytes를 묶음으로 보존하고, 호스트와 trust를 각각 새 비활성 경로에 추출했습니다. Host manifest hash와 trust receipt가 일치하고 모든 원본 host 파일 hash 및 current trust/Prepared intent가 유지됐습니다. 새 encrypted archives·추출·실패 staging은 private ignored 경로에 보존하며 자동삭제하지 않습니다. 실제 fixture는 local Docker cached ARM64 개발 환경이고 운영·Windows 복원 qualification이 아닙니다.


## 현재 복원점 identity 검증과 executor용 opaque proof

```sh
exhibitos-update verify-current-checkpoint-pair \
  --profile /absolute/private/profile --installation default \
  --host-archive /absolute/private/pair/host.bin \
  --trust-archive /absolute/private/pair/trust.bin \
  --key /absolute/external/key.bin \
  --pair-binding /absolute/private/pair/pair-binding.bin --apps-closed
```

이 명령은 원래 host 폴더가 존재하는 상태에서 사용할 수 있다. journal 전환을 노출하지 않는 `CheckpointVerifier`가 현재 exclusive authority를 유지하며 AES-GCM pair binding의 namespace·generation·head·선택적 source plan을 확인하고, 두 archive의 전체 ciphertext를 제한된 버퍼로 해시한다. 같은 실행에서 다시 검사한다. 추출 사본·새 snapshot·Engine 실행·journal 전환을 만들지 않는다. 같은 namespace라도 현재 generation/head와 다른 과거 pair는 거부한다. 별도 domain/키, 변경된 ciphertext, 불명확한 입력 권한도 거부한다. 실제 검증 범위는 Unix이며 Windows는 컴파일과 native qualification을 구분한다.

Rust의 `Store::verify_checkpoint_pair`는 외부 JSON으로 생성할 수 없는 `VerifiedCheckpointPair`를 반환한다. `receipt()`는 진단용 hash/크기와 현재 generation/head를 출력할 뿐이다. Executor는 `Store::recheck_checkpoint_pair`로 동일한 opaque proof와 현재 입력을 재검사해야 한다. 서로 다른 pair를 끼워 넣거나 Store authority가 바뀌면 재검사가 실패한다. 이 타입은 전체 데이터 복원, live authority 복구, 릴리스 유효성·호환성·공간 또는 Applying 권한을 대신하지 않는다. `sourcePlanBound=false`인 일반 host/trust pair를 현재 DB/blob 관측 결과로 취급하지 않는다. 실제 inactive host/trust 추출은 별도 CLI와 exact 파일·권한·이력 대조를 수행한다.

읽기 전용 verifier는 진행 중인 Applying 등의 intent를 검사 목적으로 열어도 Interrupted 기록을 추가하지 않는다. 반대로 정상 `Store::open`의 crash recovery는 그대로 동작한다. Verifier가 살아 있는 동안 다른 Store는 잠금으로 거부되며 verifier는 mutable Store를 공개하지 않는다.
