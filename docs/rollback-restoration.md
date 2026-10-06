# 원래 백업으로 새 롤백 후보 복원

Unix 개발 코어 `Store::restore_registered_rollback_candidate`는 같은 Store가
`register_rollback_candidate`로 예약한 새 공간에 원래 계획의 암호화 백업을 복원합니다.
원래 백업 ID·manifest hash·전체 inventory·migration 포함 schema·Runtime image를
변경할 수 없습니다. 외부 writer 정지 확인은 호출 시 필요합니다.

프로필·원본·변경 대상·새 후보의 잠금과 폴더 식별자를 유지하고, 기존 파일이 있는
후보를 덮어쓰지 않습니다. 암호화 검증과 실제 PostgreSQL·작품·설정·이미지 복원,
인벤토리 비교·원래 Runtime 실행이 성공해야 `AwaitingRollbackHealth`로 이동합니다.
원본의 다섯 설치 설정 파일도 인증된 백업 manifest와 다시 대조합니다.
호출자가 만든 성공 receipt나 boolean으로 이 이벤트를 허용하지 않습니다.

출력 receipt는 상태 설명용이며 활성화 권한이 아닙니다. 이 단계는 원래 선택을
유지합니다. 실패 시 새 후보·볼륨·기록을 보존하고 복구 실패를 기록합니다.
원래 Runtime 건강 재관측·선택 전환·authority 완료·changed migration 전체 복구·
cold/crash 복구·Windows 및 GUI는 후속 검증이 필요합니다.

## 용량과 실제 검사

기존 암호화 archive를 읽어 쓰며 새 전체 host checkpoint 사본을 만들지 않습니다.
추가 추출/이미지 용량과 계획의 여유 기준,6GiB disk floor를 검사합니다.
실제 복원을 검사하는 합성 Docker fixture는 별도 새 프로필·후보를 만들고 원래
백업/키/서비스를 보존합니다. 검사 종료 시 새 후보만 정지하고 후속 건강/선택 검사에
필요한 공간을 보존합니다. 이 검사는 합성 실패 journal을 사용하므로 실제 업데이트
적용·마이그레이션 실패·전체 롤백 활성화의 증거로 계산하지 않습니다.

일반 검사는 `cargo test -p exhibitos-lifecycle --lib rollback_restoration --locked`입니다.
Engine 검사는 기본으로 ignored이며 원래 합성 백업과 계획을 명시적으로 지정한
격리 개발 환경에서만 실행합니다. 실제 사용자 데이터에 적용하는 절차가 아닙니다.
