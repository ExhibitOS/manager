# Rollback 후보 공간 준비

`Store::register_rollback_candidate(true)`는 현재 `RecoveryRequired` 계획에 새 비활성 복원 후보를 등록한다. 기존 업데이트 대상 또는 실패 후보에 데이터를 덮어쓰지 않고, 원래 권한 기록에 새 UUID를 먼저 예약한 뒤 빈 private 디렉터리와 registry 항목을 만든다. 큰 backup·DB·image 사본은 만들지 않는다.

## 기록 순서와 중단

1. 현재 Store/독립 recovery vault·profile anchor·registry와 활성 선택이 확인된다.
2. 원본 source와 target의 operation lock을 보유한다. 입력으로 임의 후보 ID·경로·계획을 받지 않는다.
3. 같은 권한 namespace에 `BeginRestore`를 영구 기록한다. 단계는 `Restoring`이며 아직 데이터를 복원한 상태가 아니다.
4. 새 전용 공간을 만들고 부모·후보 inode와 빈 상태를 다시 확인한다.
5. 기준 registry가 그대로일 때만 새 recovery 항목을 추가한다. 활성 선택은 보존한다.

예약/생성/registry 기록 뒤 프로세스가 중단되면 Store 재열기는 `RecoveryRequired`로 기록한다. 이전 ID는 계속 예약되며 다음 시도는 새로운 공간을 만든다. 실패한 폴더·등록 항목·이전 immutable 기록은 보존한다. 폴더 교체·외부 bytes·선택 변경·동의 누락·다른 stage는 거부한다. 에이전트가 실제 실패 자료를 조사하기 전 자동으로 정리하지 않는다.

## 증거와 후속 연결

5개 실제 파일 기반 검사는 예약·선택·이력 보존, 세 중단 지점, 외부 bytes·폴더 교체 거부를 확인한다. 이 중 부모 검사가 새 합성 namespace마다 별도 자식 프로세스를 실행해 exit77로 실제 종료시키고 Store를 재열어 검사한다. Ignored worker는 이 부모 검사에서3회 실행된다. 이는 이 등록 단계의 crash 검증이다.

반환 객체는 등록 결과이며 복원 실행 권한이나 health proof가 아니다. `runtimeDataRestored`, `runtimeStarted`, `activated`는 모두 false다. 현재 public Rust API만 추가되었고 CLI/GUI에 연결되지 않았다.

다음에는 현재 계획의 원래 backup ID/manifest/inventory/schema/image를 새 공간의 실제 암호화 복원·원래 Runtime health 관찰에 연결해야 한다. 전체 rollback completion·선택 활성화·변경 migration·cold/whole-profile/authority-loss 복구·Windows native 검증은 별도 완료 기준으로 남는다. 이 등록 검사로 실제 개발 전시 업데이트의 가역성을 선언하거나 기존 승인 거절을 우회하지 않는다.
