# 만료된 개발 계획 갱신과 비활성 후보 등록

이미 수락한 서명 envelope가 만료되면 현재 신뢰 정책으로 적용 전에 재검증할 때 거부된다. 기존 journal을 삭제하거나 version/time floor를 낮추거나 사용한 ID를 다시 준비하지 않는다. 개발 서명자의 private key가 사라졌다면 권한 있는 운영자가 별도 개발 공개키 정책을 교체할 수 있다. production publisher/계정 권한을 제공하는 기능은 아니다.

## 비활성 새 후보

```sh
exhibitos-update register-update-target \
  --profile /absolute/private/profile --installation default \
  --preserve-active --apps-closed
```

현재 Store anchor를 유지하고 exclusive profile session과 profile/source operation lock을 잡는다. 등록된 원본과 Prepared source ID를 검사하며 임의 경로·외부 ID는 입력받지 않는다. 새 UUID의 private recovery directory와 registry/history만 생성하고 active ID·선택 목록의 기존 항목·원본·이전 후보를 보존한다. 결과의 targetInstance를 새 계획에 사용한다. activated/runtimeStarted는 false다. 신뢰 floor, intent 또는 Engine을 바꾸지 않는다. 기존 registry 없이 시작하거나 누락 원본을 재생성하지 않는다. Prepared 이외의 실행 intent가 있으면 거부한다.

이 등록은 restore/update 성공이 아니다. 기록 저장 실패 뒤 private orphan/staging이 남을 수 있으며 자동 삭제하거나 활성화하지 않는다. 정상 등록 결과를 확인한 뒤 다음 명시적 단계에 사용한다.

## 명시적 갱신 순서

1. 실제 현재 Prepared, 신뢰 policyGeneration/journal generation과 정책 floor를 읽고 보존한다. 원래 artifact의 실제 bytes/hash와 이전 후보/서비스 archive도 보존한다.
2. 새 비활성 후보를 등록한다. 새 개발 공개키 정책은 channel/target/protocol과 높은 floor를 유지한다. 제거한 키 ID는 영구 revocation에 남는다. 정책 교체는 별도 운영자 동작이며 release feed로 자동 신뢰하지 않는다.
3. exact current generation과 이전 operationId로 `discard-update-intent --preserve-data`를 호출한다. Prepared pointer만 journal event로 해제하고 이전 기록·데이터·floor·ID 예약을 보존한다.
4. 높은 sequence와 최신 issuedAt/유효 expiresAt의 새 개발 envelope, 새 operationId/targetInstance 및 검증된 실제 artifact로 `prepare-update`를 호출한다. 현재 source/schema/image와 원래 인증 backup ID/manifest/inventory를 정확히 바인딩한다.
5. 새 후보에서 실제 복원을 수행하고 새 계획에 맞춰 source/compatibility/recovery/resource 검사 및 mutation 직전 현재 trust/time을 다시 확인한다. 이전 target-bound 결과를 새 target의 성공으로 옮기지 않는다.

갱신은 여러 명시적 명령이다. 도중 실패·중단이면 실제 current journal을 다시 읽고 해당 단계의 기록을 확인한다. 자동 Applying이나 journal rollback은 하지 않는다. 새 서명 시간 만료 전 완료되지 않았다면 새 정책/time 검증을 다시 수행한다.

## 실제 개발 fixture 검사

```sh
node scripts/test-development-renewal.mjs \
  /absolute/exhibitos-update /absolute/private/profile \
  /absolute/runtime.tar /absolute/private/output-parent \
  --local-development-fixture
```

이 스크립트는 만료된 development Prepared fixture만 대상으로 한다. Ed25519 private key는 실행 프로세스 안에서만 사용하며 파일로 내보내지 않는다. 공개 policy/envelope와 단계별 current 관측을 새 private 경로에 보존한다. 메모리 전체의 안전한 zeroization을 보장한다는 뜻은 아니다. 실제 만료 거부, 낮은 floor 정책 거부, 원본 active/기존 registry 보존, 키 revocation, old ID 재사용 거부, 실제 artifact로 새 higher-floor Prepared를 검사한다. 동일 invocation의 자동 재실행/production signing/새 서비스 복원/실제 업데이트 완료 기능이 아니다. 마지막 report의 candidateRestored/preflightVerified/updateExecuted는 false다.

지원 OS/권한과 정책 보존 한계는 [trust 기록](release-trust.md), 실제 적용 조건은 [update intent](update-intent.md)를 따른다.

## 새 계획에 묶인 실제 후보 복원

갱신 report와 실제 등록된 비어 있는 새 target을 다음 검사에 사용한다. 다른 target의 이전 receipt를 복사하지 않는다.

```sh
python3 scripts/test-renewed-candidate.py \
  --cli /absolute/exhibitos-update --manager /absolute/exhibitos-manager \
  --renewal-report /absolute/private/renewal/report.json \
  --archive /absolute/private/authenticated-service-archive \
  --key /absolute/external/key.bin \
  --maintenance-image sha256:<qualified-maintenance-image> \
  --local-development-fixture
```

실제 artifact hash/크기·개발 release 유효시간·현재 Prepared·비어 있는 등록 target을 먼저 검사한다. 기존 profile 파일의 bytes/권한, archive/key hash 및 기존 container 상태를 보존 관측하고 CLI로 실제 암호화 archive를 새 target에 복원한다. Backup/schema/image/inventory binding과 실행 상태를 검사한 뒤 **새 후보만 명시적으로 정지**한다. 기존 원본·profile intent/floors·이전 후보는 유지한다. 실패하면 새 private 검사 공간과 실패 후보를 보존하며 자동 삭제·재시도·정지를 하지 않는다. 같은 target에서 재실행하면 비어 있는 target 조건에서 거부한다.

결과는 새 source-version 후보 복원이다. Target-version image 적용·migration·full-data rollback·통합 사전 검증은 포함하지 않으며 `preflightVerified`와 `updateExecuted`는 false다. Container 상태 비교만으로 기존 모든 volume bytes 보존을 증명하지 않는다. 유지관리 image는 별도로 검증된 immutable digest여야 하며 이 검사 도구가 production release나 운영 권한을 제공하지 않는다.
