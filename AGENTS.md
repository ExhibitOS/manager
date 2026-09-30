# manager 에이전트 작업 규칙

## 담당 범위

Windows/macOS에서 설치, runtime 시작·종료, 상태 확인, 백업·복원, 업데이트·rollback을 제공한다.

spec의 versioned OED와 platform의 공개 lifecycle API/CLI·health 계약을 소비한다.

## 시작과 작업

1. 이 파일, README, 현재 task의 명세와 선행 작업 evidence를 읽는다. 운영 접근권한이 있으면 operations의 구현 roadmap 및 contracts/security/quality 기준도 확인한다.
2. `git status --short`, remote와 현재 브랜치를 확인한다. 다른 작업자의 변경을 보존하고 `codex/<작업명>`에서 작업한다.
3. T08-01 → T08-02 → T08-03; cloud wizard는 T10-04.
4. 계약 변경은 version·호환성·consumer 영향과 migration 방법을 기록한다. operations/state.json의 상태 변경은 총괄 담당자와 조정한다.

## 공개·보안 경계

이 저장소는 공개 후보이며 현재 GitHub에서는 비공개다. operations 및 Capture 저장소는 빌드·설치·CI 의존성이 될 수 없다. private submodule, private package와 secret을 필수 조건으로 추가하지 않는다. 현재 프로젝트 소유 자료는 README와 LICENSE의 Apache-2.0 범위를 따른다. 공개 전환과 향후 라이선스 변경은 별도 기록과 검토 후 수행한다.

인증은 OS credential store 또는 환경별 secret store로 관리한다. 토큰, signing key, 원본 작품·사진·개인정보는 Git이나 로그에 기록하지 않는다. fixture는 synthetic 또는 재배포 권리를 확인한 자료를 사용한다. 실제 데이터 삭제·운영 변경은 복구를 검증한 가역적 방식만 사용한다. 신규 유료 서비스는 비용 확인과 전체 월 10,000원 예산 통제가 필요하며, 이 준비 단계에서는 사용하지 않는다.

## 검증과 완료

현재 제품 구현, CI, test suite와 toolchain은 없다. 없는 build/test가 통과했다고 보고하지 않는다. T00-02에서 실제 검사 명령을 README에 추가한 뒤 각 변경에 해당하는 검사를 수행한다. 계획된 검사: fresh install, start/stop 재시도, 중단된 update, backup/restore, version mismatch와 rollback을 검증한다.

문서 변경은 `git diff --check`, 링크/예제와 tracked tree를 확인한다. 코드가 추가되면 정상·실패·권한·재시도 경로를 task acceptance에 맞춰 검증하고 환경·명령·결과·제한을 남긴다. 기기·서명 검증은 실물 증거 없이 완료 처리하지 않는다.

PR에는 문제, 변경, 검증, 남은 조건을 적고 검토 후 merge한다. reset --hard, clean, force push를 기본 복구로 쓰지 않는다. 보안 보고 경로는 README를 따른다.
