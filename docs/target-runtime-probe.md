# 독립 사본의 target Runtime 검사

개발용 `scripts/probe-target-runtime.py`는 genuine update plan의 완료된 synthetic 복원 후보를 원본으로 사용합니다. 원본 컨테이너가 정지됐는지 확인하고 DB·작품·설정을 새 고유 볼륨에 읽기 전용 원본에서 복사합니다. 바이트·권한·소유자를 대조한 뒤 계획에 선언한 실제 target image를 사본에서 실행합니다.

```sh
python3 scripts/probe-target-runtime.py --fixture /absolute/private/genuine-fixture --cli /absolute/exhibitos-update --docker /absolute/docker
```

Python 최적화 옵션은 거부합니다. Fixture에는 `genuine-release-binding.json` 및 완료된 profile-1 복원 후보가 있어야 합니다. 로컬 maintenance/source/target 이미지가 필요하며 pull하지 않습니다. 최소 6GiB 여유 공간을 요구합니다. 복사 한도는 2GiB/200,000 entries/depth64이고 symlink·hardlink·special file을 거부합니다.

실제 target image/protocol1 readiness, 재시작 0회, synthetic DB witness와 blob, 보존된 관리자 로그인 및 웹 응답을 검사합니다. Target 컨테이너를 정지한 다음 이전 Runtime을 동일한 사본에서 실행하여 readiness·witness·로그인·웹 응답을 검사합니다. 마지막에 이 검사 프로젝트만 정지하며 원본 설치 파일·컨테이너 상태·Prepared intent 보존을 대조합니다.

모든 새 볼륨·helper·컨테이너·private env·실패 사본을 보존합니다. 삭제 도구가 아니며 private 파일이나 로그를 Git에 넣지 않습니다. 중단 시 고유 프로젝트를 확인하고 해당 서비스만 정지해야 합니다. 원본 설치를 자동 재시작하거나 변경하지 않습니다.

2026-10-04 macOS ARM64/Docker 실제 실행에서 세 볼륨 복사, target 기본 흐름, 이전 Runtime 기본 흐름이 통과했습니다. 이는 개발용 독립 사본 검사입니다. 전체 schema/data inventory 비교, 실패·중단 경로, cold engine, Windows/Podman, 등록된 설치 health receipt, 최신 trust/time/source/resource/security recovery 통합, 실제 apply/migration/rollback은 완료되지 않았습니다. 관리자·외부 writer로부터의 완전한 격리도 입증하지 않습니다. Docker 명령 응답 크기는 완료 후 확인하므로 악의적인 daemon 응답의 메모리 사용을 완전히 제한하지 않습니다.

후속 probe는 세 단계의 전체 stopped-copy 인벤토리를 기록합니다. 신규/이전 Runtime을 각각 정지한 뒤 관측하여 DB identity/schema/migration/non-session tables/blob/reference 보존과 로그인당 정확히 한 세션 행 추가·기존 세션 행 hash 보존을 비교합니다. 상세 결과와 제한은 [전체 인벤토리](target-inventory.md)를 참고하세요.

## 실제 프로세스 중단 검사

`--interrupt-target`는 위의 새 synthetic 사본에서만 target Runtime에 SIGKILL을 보내 종료137을 확인한 후 같은 컨테이너를 명시적으로 재시작합니다. Readiness 복구 후 Runtime을 정지하고 copied PostgreSQL에 SIGKILL을 보내 종료137을 확인한 다음 DB와 Runtime을 다시 시작합니다. 정확한 관측 container ID의 새 고유 project/service/compatibility label이 일치해야 하며 원본 컨테이너는 종료하지 않습니다.

회복한 readiness와 DB witness를 확인한 뒤 서비스 정지·전체 인벤토리 비교·이전 Runtime 기동과 후속 인벤토리를 수행합니다. 원본 파일/states/Prepared intent와 새 사본/helper 보존 경계는 동일합니다. PostgreSQL 사본의 crash recovery는 실제 쓰기를 수행하며 원본은 별개로 정지 상태를 유지합니다. 이것은 개발 fixture의 process interruption 검사이며 물리적 host 전원 손실, fsync 손실, migration 중단, 업데이트 Applying journal 복구나 실제 설치 rollback qualification은 아닙니다.

## 갱신된 현재 계획으로 검사

개발 계획 갱신 후 이전 target의 검사를 새 target 성공으로 재사용하지 않는다.
새 후보를 실제 복원·정지한 뒤 현재 Prepared와 정확히 같은 private 계획을 지정한다.

```sh
python3 scripts/probe-target-runtime.py \
  --fixture /absolute/private/development-fixture \
  --cli /absolute/exhibitos-update \
  --docker /absolute/docker \
  --plan-file /absolute/private/renewed-plan.json \
  --interrupt-target
```

계획은 owner의 regular single-link400/600 파일,16KiB 이하, canonical 절대 경로다.
중복/알 수 없는 JSON 필드를 거부한다. 실제 Store의 current Prepared와 exact plan이
일치해야 후보 경로에 접근하거나 Engine resource를 만들 수 있다. 검사 후 같은 파일
identity/bytes와 계획을 다시 확인하고 report에 plan·file SHA를 기록한다.
기존 fixture의 서명/계획 파일은 덮어쓰지 않는다.

이 경로도 새로운 독립 synthetic 복사본의 Runtime/전체 inventory 관측이다.
registered target에 target image를 적용하거나 활성 전시를 전환하는 명령이 아니며,
전체 preflight·migration·health 실패·복원 실패·실제 rollback 인수 조건은 남는다.
