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
