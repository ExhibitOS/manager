# 롤백 선택과 신뢰 기록의 일관성

예약된 새 후보의 복원이 `AwaitingRollbackHealth`에 도달하면, 롤백은 원래
계획의 이미지·migration 포함 schema와 그 후보 ID에 대한 실제 건강 관측을
요구합니다. 선택 전환은 같은 프로필과 같은 authority namespace를 유지합니다.

이번 journal 코어는 업데이트 journal과 별도의 `<operation hash>-rollback`
경로에 원래 선택 bytes, 후보 선택 bytes, 계획·관측·이전 authority head를
기록합니다. 이전 업데이트 journal 형식은 바꾸지 않습니다. 후보 선택을 먼저
게시하고 정확히 다음 authority generation의 `RolledBack` 건강 이벤트가
확인된 경우에만 완료 marker를 기록합니다. 공간·이력·기존 후보를 삭제하지 않습니다.

중간 종료 후 `Store::reconcile_rollback_selection_activation`은 명시적인
metadata 복구만 수행합니다. 완료 건강 이벤트가 이미 저장돼 있으면 marker를
마칩니다. 저장되지 않았다면 실패를 기록하고 정확한 이전 선택 bytes로 복구합니다.
원래 프로필·원본·변경 대상·복원 후보 잠금과 식별자를 유지하며, unknown 선택,
변조된 journal과 경쟁 작업을 거부합니다. Runtime 시작이나 건강 검사를 재생하지 않습니다.

## 검증 범위와 남은 연결

합성 단위 검사에서 원래 이미지/schema/candidate binding, 정확한 이전 선택
복구, 같은 authority 유지, 잘못된 관측과 unknown 선택·경쟁 잠금 거부를 확인합니다.
별도 자식 프로세스를 준비·선택 게시·건강 기록 경계에서 실제 exit77로 종료한 뒤
재열기와 metadata 복구를 검사합니다. 이 관측은 합성 신뢰 어댑터 입력입니다.

실제 Runtime 건강을 다시 관측하고 이 private journal preparation을 호출하는
native 롤백 실행기의 연결은 아직 남아 있습니다. preparation은 공개 receipt
endpoint가 아니며, GUI에서 받은 boolean으로 호출할 수 없습니다. 실제 적용·변경된
마이그레이션·전체 cold/crash 복구·Windows 인수 검증까지 통과해야 전체 롤백을
완료로 판정합니다. 기존 실제 개발 전시 업데이트의 가역성 검증 조건은 유지합니다.
