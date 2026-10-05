# 같은 실행 범위의 원본·호스트·신뢰 기록 체크포인트

이 개발 CLI는 기존 Prepared 계획과 등록된 복원 후보를 사용해 현재 원본 DB/blob·설정·이미지를 기존 인증 서비스 백업에 대조한다. 이 관측과 호스트·신뢰 기록의 암호화 사본을 하나의 cooperative fence에서 수행한다. 새 외부 서비스 volume 백업 생성, live authority 복원, 업데이트 실행은 포함하지 않는다.

```sh
exhibitos-update checkpoint-source-host-trust \
  --profile /absolute/private/profile --installation default \
  --image sha256:<trusted-maintenance-image> \
  --key-file /absolute/external/key.bin \
  --destination /absolute/external/new-pair \
  --external-writers-quiesced --host-writers-stopped --apps-closed
```

키는 profile/trust root 밖의 private regular file이며32 bytes다. 출력은 기존 private parent 아래의 사용하지 않은 절대 경로이고 기존 사본을 덮어쓰지 않는다. 실제 검증 범위는 Unix/Docker 개발 환경이며 Windows qualification은 별도 조건이다.

## 실행과 관측

1. Store의 기존 trust/pathname anchor와 ExecutionSession의 exclusive legacy session을 유지한다. 호스트 처리는 같은 opaque session을 빌려 profile/root identity를 검사한다. 재잠금·잠금 downgrade는 하지 않는다.
2. profile operation과 사용 가능한 모든 등록 installation root의 operation lock을 잡는다. 원본·복원 후보를 모두 포함한다.
3. DB/blob, 설정7개·원본 image archive2개, 반복 DB/blob을 인증 백업에 대조한다. 새 원본 image export는 호스트 inventory 전에 생성하므로 호스트 사본에 포함된다.
4. 호스트 전체 파일과 committed trust history를 각각 암호화한다. 같은 잠금 범위에서 DB/blob, 설정7개·image bytes2개를 다시 관측한다. 후반 image export는 profile 밖의 private staging에 놓아 호스트 inventory를 바꾸지 않는다.
5. 원본 데이터 대조와 호스트 재inventory가 일치할 때만 새 pair를 공개한다. 실패 staging은 private하게 보존하며 기존 데이터를 삭제하지 않는다.

결과는 `host`, `trust`, `source`, `afterArchiveInventory`, `afterArchiveConfiguration`을 담는다. `verified.json`은 private 운영 관측이며 신뢰하지 않는 입력을 업데이트 권한으로 바꾸는 인증서가 아니다. 후반 exportWorkspace는 공개된 pair 안의 최종 경로를 가리킨다.

## 복원 검사와 남은 조건

`host.bin`은 [host checkpoint](host-checkpoint.md) CLI로 별도의 inactive directory에 추출한다. `trust.bin`은 [trust 기록](release-trust.md) CLI로 같은 current Store의 전체 이력과 대조하며 inactive directory에 추출한다. 자동 activation은 하지 않는다.

`externalVolumesSaved`, `liveAuthorityRestored`, `preflightVerified`, `updateExecuted`는 false다. 현재 서비스 데이터 일치는 기존 백업에 대한 관측이며 DB/blob/config live volume을 새 archive에 저장했다는 뜻이 아니다. 원래 인증 서비스 archive를 별도로 보존하고 전체 일관된 복원, authority-loss, 등록된 실제 업데이트·migration·health·rollback, cold engine/OS/corpus 검사를 완료해야 한다. cooperative lock과 operator acknowledgement는 privileged external writer를 배제하지 않는다.

개발 서명 envelope가 만료됐어도 이 관측으로 Applying을 허용하지 않는다. 실제 업데이트 전에 서명·시간·floor/revocation/history·source/recovery/resource를 새로 검사해야 한다.

## 누적 공간 사전 검사

단계별 여유가 있어도 앞 단계의 새 image export·host archive가 남으면 뒤쪽 반복 DB inventory에서 공간이 부족해질 수 있다. 같은 profile/모든 등록 root 잠금을 확보한 첫 callback에서 bulk 작업 전에 지원 host inventory를 실제 hash/byte 검사하고, 현재 계획에 바인딩한 인증 backup의 두 image archive 크기 및 검증된 전체 trust history의 ciphertext 예산을 계산한다. 잘못된 manifest·history·owner/path·quota는 성공 예산으로 처리하지 않는다.

`H`는 기존 host profile bytes, `I`는 두 인증 image archive 합계, `T`는 trust ciphertext 예산이다. 원본과 목적지가 같은 Unix filesystem이면 `H + 3I + T + 268MiB + 6GiB` 이상을 요구한다. 첫 export `I`는 원본에 남고 host archive에도 포함되며, 두 번째 export `I`는 외부 목적지에 남는다. 서로 다른 filesystem이면 원본에는 `I + 6GiB`, 목적지에는 `H + 2I + T + 268MiB`를 각각 요구한다. Host archive64GiB/manifest8MiB quota도 기존 profile에 첫 export를 더한 크기로 먼저 검사한다. `CHECKPOINT_STORAGE_INSUFFICIENT`는 새 DB snapshot·image export·암호화 archive 전에 거부하며 빈 private 진단 stage만 남을 수 있다.

성공 receipt의 `storage`는 측정 bytes·required/available·filesystem 공유 여부를 기록한다. 기존 receipt의 추가 필드이며 archive/wire 포맷은 바꾸지 않는다. 이는 **알려진 host/export 쓰기의 누적 예산**이다. Docker VM/Engine volume의 DB 복사 용량을 예약하거나 계산하지 않고, unrelated writer·filesystem overhead·sparse/compression/allocation 변화도 통제하지 않는다. `engineVolumeSpaceReserved`와 `diskSpaceReserved`는 false다. 기존 반복 DB snapshot6GiB 검사, 실제 host archive 공간 검사, image export 및 snapshot의 개별 byte quota는 그대로 유지한다. 초기 예산 통과도 이후 실패 가능성을 제거하거나 Applying 권한을 만들지 않는다.

현재 실제 검증 범위는 낮은 공간에서의 사전 거부, 같은 borrowed fence의 host 측정/암호화 archive 일치 및 arithmetic 경계 검사다. 충분한 공간에서 전체 새 source-host-trust checkpoint와 비활성 추출·coherent recovery·실제 업데이트 검사는 별도로 필요하다. Windows filesystem/host checkpoint qualification은 그대로 남아 있다.
