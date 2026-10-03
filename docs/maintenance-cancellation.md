# 진행 중 백업·복원 취소

현재 macOS·Docker 백업 생성과 새 설치 복원 작업은 앱의 **진행 중 백업·복원 취소**에서 중단을 요청할 수 있습니다. 후보 보존 동의를 확인하고 **작업 취소 요청**을 누르세요. 다른 설치·시작·복원 요청과 관리 공간 전환은 진행 중에 차단됩니다.

**취소 요청됨은 정지 확인이 아닙니다.** 요청은 private 작업 UUID·종류·현재 선택 토큰에 연결됩니다. worker가 operation lock을 계속 보유한 상태에서 처리하며, 실행 중 helper를 실제로 관찰하고 정확한 ID·image·이름·label·mount를 재확인한 뒤 정지합니다. 이미지가 선언한 보조 volume은 named storage 또는 기존 익명-volume anchor로 보존합니다. 새 복원 서버가 생성됐다면 해당 후보의 소유권과 실제 정지도 확인합니다. 원본 서버를 자동 재개하거나 DB·blob·키·사본·실패 후보·volume을 지우지 않습니다.

이미지 저장·불러오기, 파일 hash/복사, Compose 설치·시작 등은 현재 bounded 명령이 끝난 뒤 다음 안전한 지점에서 요청을 처리할 수 있습니다. 요청 직후 모든 엔진 명령이 즉시 중단되는 것은 아닙니다. helper가 아직 나타나지 않았거나 시작 전인 동안에는 attach client를 죽이고 부재를 성공으로 추측하지 않습니다. helper 실행이 실패하거나 engine 조회·정지·소유권 확인이 불확실하면 `CANCEL_UNCERTAIN`이며 확인 완료로 표시하지 않습니다.

정지 확인 후 원래 백업·복원 journal은 `interrupted`/`CANCELLED`, 취소 기록은 `confirmed`가 됩니다. 성공 receipt를 생성하지 않습니다. 작업 완료와 요청은 별도 private cancellation lock으로 최종 기록까지 직렬화합니다. 완료가 먼저 확정되면 늦은 취소는 거부하고, 요청이 먼저 저장되면 worker는 정지 확인 없이 완료를 기록하지 않습니다. 이미 실패한 엔진 실행을 늦은 요청으로 취소 성공으로 바꾸지 않습니다. 백업을 취소하기 전 Platform을 정지한 경우에도 자동 재개하지 않습니다. 쓰기 중지 이전 단계에서 취소하면 기존 서버가 계속 실행 중일 수 있으므로 실제 상태를 확인하세요.

앱·프로세스가 중단된 뒤 재열기 시 `interrupted`는 취소 성공이 아닙니다. 남은 helper는 [실패·중단 helper 확인](helper-reconciliation.md)으로 확인하세요. 새 복원 후보는 일반 시작·다시 시도로 재개할 수 없습니다. 새 관리 공간에서 보존한 사본으로 다시 복원합니다. 백업 재시도도 남은 helper 정지와 외부 writer 중지를 다시 확인하고 새로운 UUID 작업을 만듭니다.

## CLI와 비공개 기록

```sh
exhibitos-manager --root '<current canonical private root>' maintenance-context
exhibitos-manager --root '<same root>' cancel-maintenance \
  restoration '<active job UUID>' --preserve-candidates
exhibitos-manager --root '<same root>' cancel-maintenance \
  backup '<active job UUID>' --preserve-candidates
```

`maintenance-active.json`은 현재 작업 UUID만 가리키고 `maintenance-<UUID>.json`에는 종류·상태·단계·오류 코드·시간을 0600으로 저장합니다. `maintenance-cancel.lock`도 0600이며 symlink·다중 hardlink·다른 소유자·완화된 권한을 거부합니다. 요청에는 실제 보유된 operation lock과 private 원래 진행 journal이 필요합니다. 기록은 atomic replacement/file sync 및 Unix parent-directory sync를 사용합니다. 실제 powerloss 복구까지 검증됐다는 뜻은 아닙니다. 외부 key bytes·credentials·원본 artwork·raw engine logs는 저장하지 않습니다. 이 파일들과 helper/volume/candidates는 Git 백업 밖이며 일관된 runtime/profile backup과 별도로 보존해야 합니다.

## 실제 검사와 한계

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --locked -p exhibitos-lifecycle --bin exhibitos-manager
python3 scripts/test-maintenance-cancellation.py --kind restoration \
  --manager '<built CLI>' --fixture '<retained synthetic backup fixture>'
python3 scripts/test-maintenance-cancellation.py --kind backup \
  --manager '<built CLI>' --fixture '<retained synthetic backup fixture>'
```

두 실제 engine 검사는 순차 실행합니다. 새 synthetic helper의 Node PID에만 SIGSTOP을 보내 실행 중인 지점을 고정하고 실제 취소 요청·정지·journal·volume witness·키/사본/설정 hash·원본 서비스 상태를 확인합니다. 백업 검사는 이전에 정지된 synthetic Runtime만 잠시 시작하고 종료 후 원래 정지 상태로 복구합니다. 새 후보와 volume은 자동 삭제하지 않습니다. 단위의 합성 engine 응답·브라우저 synthetic IPC·실제 Docker/CLI·네이티브 GUI를 구분합니다. 실행 결과가 확인된 경우에만 증거로 기록합니다.

기본 설치/시작/정지 및 검증 전용 사본 작업의 취소, Windows·Podman, cold-engine·전체 frozen corpus/OEX, 실제 native GUI와 서명·공증은 별도 검증/구현 범위입니다. 이 기능으로 T08-02 전체 acceptance를 완료 처리하지 않습니다.

## 중단·소유 정보 충돌 검사

동일한 합성 fixture에서 다음 두 검사를 각각 순차 실행합니다.

```sh
python3 scripts/test-maintenance-cancellation.py --kind restoration --mode crash \
  --manager '<built CLI>' --fixture '<retained synthetic backup fixture>'
python3 scripts/test-maintenance-cancellation.py --kind restoration --mode ownership-conflict \
  --manager '<built CLI>' --fixture '<retained synthetic backup fixture>'
```

`crash`는 새 작업의 CLI 프로세스만 종료한 뒤 재열기의 `interrupted`/`INTERRUPTED`와 별도 guarded helper 정지를 확인합니다. `ownership-conflict`는 새 합성 helper의 이름을 일시 변경해 소유권 불일치를 만들고 `uncertain`/`CANCEL_UNCERTAIN` 및 자동 정지 거부를 확인합니다. 정확한 이름을 복구한 뒤에만 별도 guarded 정지를 수행합니다. 두 경우 모두 별도 정지 후에도 원래 실패·중단 journal과 취소 상태를 성공으로 바꾸지 않습니다. 원본 사본·키·설정·서비스 상태와 후보 volume witness를 확인하며 후보는 삭제하지 않습니다. 이 검사들은 실제 Docker/CLI 증거이고 GUI·powerloss·Windows 검증을 대신하지 않습니다.
