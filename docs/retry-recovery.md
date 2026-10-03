# 연결되지 않은 재시도 작업 진단·복구

새 재시도에서 앱·CLI가 종료되어 새 작업 ID가 parent 기록에 남지 않은 경우의 **개발 CLI**입니다. 백업·복원 실행, 원래 데이터 복구, 네이티브 진단 화면 완료를 뜻하지 않습니다. 작업과 후보를 보존하며 자동 재시도하지 않습니다.

## 실행 절차

1. 같은 개발 소스로 빌드한 Manager CLI를 사용합니다. 해당 공간에서 실행 중인 작업이 있으면 `BUSY`가 반환되므로 그 작업이 끝날 때까지 기다립니다. 다른 프로세스를 임의 종료하거나 파일을 지워 잠금을 우회하지 않습니다.
2. 일반 상태 확인이 불완전 후보 때문에 실패해도 진단 전용 이력 조회로 재시도 UUID를 확인할 수 있습니다. 아래 `<retry UUID>`는 **원래 실패 작업 UUID가 아닌 재시도 기록의 `id`**입니다.
3. 백업 재시도에는 `--same-root`, 복원 재시도에는 기록의 목적지 hash와 일치하는 실제 별도 공간의 전체 경로를 사용합니다. 복원 목적지·원본·키·후보를 이동하거나 지우지 않습니다. CLI는 지정한 공간을 생성하거나 선택 등록을 바꾸지 않습니다.
4. 먼저 진단 결과를 읽습니다. `canReconcile: true`인 경우에만 보존 동의를 포함한 복구 명령을 사용합니다. `dataPreserved`는 이 명령이 데이터·후보를 삭제하거나 helper를 제어하지 않았다는 뜻입니다. 이전에 없어진 데이터가 복원됐다는 뜻이 아닙니다.

```sh
cargo build --release --locked -p exhibitos-lifecycle --bin exhibitos-manager
./target/release/exhibitos-manager --root '<original private root>' retry-diagnostic-history

# 백업 재시도
./target/release/exhibitos-manager --root '<same root>' diagnose-retry '<retry UUID>' --same-root
./target/release/exhibitos-manager --root '<same root>' reconcile-retry '<retry UUID>' --same-root --preserve-candidates

# 복원 재시도: 목적지에서 일반 시작 복구를 실행하지 않고 검사합니다.
./target/release/exhibitos-manager --root '<original root>' diagnose-retry '<retry UUID>' '<recorded private destination>'
./target/release/exhibitos-manager --root '<original root>' reconcile-retry '<retry UUID>' '<same destination>' --preserve-candidates
```

진단 전용 세 명령은 일반 constructor의 journal 중단 복구를 우회합니다. source/destination 작업 잠금과 경로·권한 검사를 수행하지만 원래 journal, child journal, runtime·helper·writer 상태를 바꾸지 않습니다. 작업 잠금 파일은 필요하면 생성됩니다.

| 진단 결과 | 의미와 다음 작업 |
| --- | --- |
| `child-found` | 준비·예약 hash와 목적지 journal의 UUID가 일치합니다. 명시적 복구로 그 child를 연결하고 parent를 `interrupted`로 유지합니다. 해당 공간에서 child 상태와 helper를 별도로 확인한 뒤 그 작업을 대상으로 재시도합니다. 완료·성공으로 바꾸지 않습니다. |
| `no-child-created` | 예약 이전이거나 예약한 디렉터리가 없거나 비어 있으며, 원래 journal·source 후보 목록·목적지 조건이 일치합니다. 원본 parent의 byte 사본과 최종 해시 증거를 저장하고 parent만 `failed / RETRY_NOT_STARTED`로 바꿉니다. 원래 작업 UUID로 새 재시도를 명시적으로 실행할 수 있습니다. 기존 helper 정지·소유권 검사는 그 새 실행에서 다시 수행합니다. |
| `already-linked` | 새 작업 연결이 이미 있습니다. 기존 연결과 해당 공간을 사용하고 원래 작업을 반복하지 않습니다. |
| `unproven` | 이전 버전 등 준비 증거가 없습니다. 작업 ID를 날짜·폴더 순서로 추측하거나 차단을 풀지 않습니다. 원래 기록과 후보를 보존해야 합니다. |
| `candidate-incomplete` / `child-journal-missing` | 후보 데이터나 기존 연결이 있지만 journal이 없습니다. 이 기능이 데이터를 복원하거나 후보를 지워 빈 공간으로 만들지 않습니다. 별도 복구가 필요합니다. |
| `inventory-changed` / `destination-changed` | 준비 후 source 후보 목록이나 목적지가 바뀌었습니다. 다른 작업·공간을 확인하고 그대로 보존합니다. |

UUID·공간 hash 불일치, 다른 child journal, 원래 journal hash 변경, symlink·권한 오류나 부분적으로 저장된 증거는 오류로 거부합니다. 알 수 없는 상태를 성공·정지로 표시하지 않습니다.

## 저장 순서와 호환성

새 재시도는 source의 `retry-preparation-<parent UUID>.json` (private format1)을 parent audit보다 먼저 저장합니다. 원래 journal hash, 목적지 hash와 검증된 backup workspace UUID 목록을 보관합니다. 새 candidate 디렉터리/journal 생성 **이전**에 `retry-child-intent-<parent UUID>.json` (format1)에 child UUID와 정확한 preparation bytes의 SHA256을 저장하고 fsync합니다. helper는 journal 생성과 parent 연결 후에만 실행합니다. 증거에 키·토큰·raw engine 응답·목적지 원문 경로를 저장하지 않습니다.

복구는 `retry-diagnosis-<UUID>-before.json`에 parent의 정확한 이전 bytes, 별도 diagnosis에 판정을 보관합니다. 생성되지 않은 child를 입증한 경우에는 `retry-clearance-<parent UUID>-<diagnosis UUID>.json`의 최종 parent SHA256 증거를 **먼저** durable 저장한 뒤 parent를 변경합니다. 중간 종료 시 parent의 차단이 유지됩니다. 불완전한 metadata 복원 등으로 clearance만 없는 경우도 현재 reader는 반복을 거부하며, 명시적 복구가 증거를 다시 검사할 수 있습니다. 빈 예약 폴더도 삭제하지 않습니다. 원래 job과 child job은 다시 쓰지 않습니다.

기존 public retry/history/receipt JSON은 바꾸지 않았습니다. 새 CLI 결과와 private 기록을 추가했습니다. 준비 증거가 없는 예전 parent는 진단할 수 있지만 차단 해제를 추측하지 않습니다. 이전 binary는 새 예약·clearance 규칙이나 보존한 빈 후보를 이해하지 못하므로 **새 기록이 있는 공간의 작업에는 현재 CLI·앱을 사용**해야 합니다. 명령 연결은 개발 CLI이며 진단·복구의 네이티브 IPC/UI는 후속 작업입니다.

## 실제 검증과 범위

```sh
cargo test --release --locked -p exhibitos-lifecycle -- --test-threads=1
cargo clippy --release --locked -p exhibitos-lifecycle --all-targets -- -D warnings
python3 scripts/test-retry-recovery.py --manager '<current built CLI>'
python3 scripts/test-maintenance-retry.py --kind restoration --pre-child-crash \
  --manager '<current built CLI>' --fixture '<retained synthetic fixture>' --docker '<Docker CLI>'
```

독립 Unix CLI 검사는 합성 private 준비·예약·원본·child 파일과 실제 flock을 사용하며 전시 엔진을 실행하지 않습니다. Docker 검사는 별도 합성 worker만 종료하고, 실제 reservation 이전 중단·원래 helper·후보 witness·새 복원·DB/blob/login/web/정지를 확인합니다. 모든 실제 검사의 source·후보·키·사본·실패 기록은 유지합니다. 실행 결과는 commit/evidence에서 확인합니다.

현재 프로토콜은 **프로세스 중단**을 대상으로 합니다. powerloss·storage 손실, Windows, 실제 네이티브 GUI, cold engine·전체 corpus·운영 데이터 복원은 별도 인수 조건입니다. 이 준비·예약·진단·clearance 파일과 미완료 candidate 데이터는 Git bundle, 현재 profile metadata archive 및 완료한 runtime 서비스 사본에 포함되지 않습니다. 별도의 앱 종료 상태에서 일관된 작업 기록·후보 사본이 필요하며 이 추가 백업 경로는 미구현입니다. [설정 사본](profile-backup.md)과 [전시 서비스 사본](backup-creation.md)의 별도 일관된 보존 범위를 유지하세요.
