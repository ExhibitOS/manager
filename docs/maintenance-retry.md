# 중단된 백업·복원의 명시적 재시도

현재 개발 CLI에서 실패·중단 작업의 UUID를 지정해 새 백업 작업 또는 별도의 새 복원 공간으로 다시 시도할 수 있습니다. 원래 작업을 완료로 바꾸거나 실패 후보 위에 이어 쓰지 않습니다. 로컬 앱에도 같은 코어의 재시도 화면을 연결했으며, 실제 네이티브 GUI·Windows·Podman 검증은 후속 조건입니다. 일반 `retry`는 설치 수명주기 작업이며 이 기능과 다릅니다.

## 실행 전 확인

1. 실패한 관리 공간과 원래 작업 UUID를 `backup-jobs` 또는 `restoration-status`에서 확인합니다. 진행 중·완료 작업은 재시도 대상으로 받지 않습니다.
2. 원본 사본·외부 키·실패 후보를 보존합니다. 백업은 외부 writer가 정지됐음을 다시 확인하고 전시 중단에 동의합니다. 복원은 기존 공간과 서로 포함 관계가 없는 새 빈 0700 디렉터리와 새 loopback port를 준비합니다. 키와 암호화 사본은 두 공간 밖에 둡니다.
3. 현재 source와 destination의 작업 잠금을 확보할 수 있어야 합니다. 남은 helper의 실제 이름·label·image·mount·전체 ID를 다시 검사하고 정지합니다. 소유권 충돌이나 정지 불확실성을 무시하지 않습니다. 복원 후보 서버가 만들어졌다면 그 후보의 소유권과 정지 상태도 확인합니다.
4. 재시도는 입력받은 신뢰된 immutable maintenance image와 외부 키로 원래 백업/복원 검사를 다시 수행합니다. 새 UUID·workspace·암호화 사본 또는 새로운 bundle/volume identity를 사용합니다. 이전 UUID·journal·DB·blob·volume·사본을 삭제하지 않고 원본 writer를 자동 재개하지 않습니다. 복원 성공 시 새 후보만 readiness와 로그인 등에 필요한 서비스 실행 상태로 전환됩니다.

```sh
cargo build --release --locked -p exhibitos-lifecycle --bin exhibitos-manager
./target/release/exhibitos-manager --root '<failed canonical private root>' maintenance-retries
./target/release/exhibitos-manager --root '<same root>' retry-backup \
  '<failed backup UUID>' 'sha256:<trusted maintenance image content ID>' '<external key file>' \
  --preserve-candidates --external-writers-quiesced
./target/release/exhibitos-manager --root '<failed restoration root>' retry-restoration \
  '<failed restoration UUID>' '<different fresh private root>' \
  'sha256:<trusted maintenance image content ID>' '<external key file>' '<encrypted archive directory>' \
  '<new loopback port>' --preserve-candidates --fresh-installation
```

## 상태와 중단 복구

source의 `maintenance-retry-<UUID>.json`은 0600 비공개 기록입니다. 대상 UUID·종류·이전 journal SHA256·새 root 경로의 SHA256·새 작업 UUID·상태·안전한 오류 코드·시간만 저장합니다. 키 내용·credentials·원문 경로·raw engine 응답은 기록하지 않습니다. 새 작업 UUID는 실제 journal 생성 후 helper 실행 전에 source 기록에 연결합니다. source operation lock은 old helper 확인부터 새 작업의 최종 결과 기록까지 유지하고, 복원 destination lock도 동시에 유지합니다. 조회의 중단 복구와 목록 snapshot은 하나의 잠금으로 직렬화합니다.

`preparing` 또는 `running` 도중 프로세스가 종료되면 재열기 기록은 `interrupted`입니다. 새 후보가 실제로 중단됐거나 성공했다고 추측하지 않습니다. 새 작업 ID가 있는 실패나 중단된 재시도를 원래 UUID에서 다시 실행하면 `RETRY_RECOVERY_REQUIRED`로 거부합니다. 해당 새 작업의 root에서 실제 상태·helper·후보를 확인하고 그 작업을 대상으로 후속 재시도를 진행합니다. 원래 UUID로 자동 반복하거나 기록을 수동으로 지워 우회하지 마세요. source 기록의 root hash는 실제 선택한 root를 식별하는 보조 정보이고 root 자체를 복구하지 않습니다. 사용자가 준비한 root와 CLI 입력을 별도로 유지하세요.

이전 journal이 바뀌거나 최종 연결 기록을 저장할 수 없으면 성공 receipt를 반환하지 않습니다. 새 후보는 남을 수 있으므로 확인하고 보존해야 합니다. retry의 `completed`와 결과 receipt는 새 작업의 결과만 뜻하며 원래 실패 journal은 그대로 남습니다. source·destination·기록·사본은 Git bundle 밖입니다. [서비스 백업](backup-creation.md)과 [설정 백업](profile-backup.md)을 각각 일관되게 보관하세요.

## 실제 검사와 범위

```sh
cargo test --release --locked -p exhibitos-lifecycle -- --test-threads=1
cargo clippy --release --locked -p exhibitos-lifecycle --all-targets -- -D warnings
python3 scripts/test-maintenance-retry.py --kind restoration \
  --manager '<built Manager CLI>' --fixture '<retained synthetic backup fixture>'
python3 scripts/test-maintenance-retry.py --kind backup \
  --manager '<built Manager CLI>' --fixture '<same fixture>'
```

두 engine 검사는 순차 실행하고 최소 2GiB 여유 공간이 필요합니다. 도구는 새 합성 helper만 잠시 SIGSTOP하고 해당 worker만 종료해 실제 orphan을 만듭니다. 이름 충돌에서 정지/새 작업 거부, 정상 guarded stop, 별도 새 UUID 결과·원래 journal·volume witness·사본/키/설정·기존 서비스 상태를 확인합니다. 성공 복원은 DB/blob·기존 관리자 로그인·웹 응답도 검사하고 새 서버를 정지합니다. 실패 후보와 원본은 유지합니다. 실행하지 않은 kind나 native GUI·Windows·powerloss·cold-engine·전체 frozen corpus·signed update를 통과로 표시하지 않습니다.

이미 실제 중단으로 생성되고 helper 정지가 확인된 합성 백업 후보를 재사용할 때만 `--kind backup --reuse-backup-target <UUID>`를 지정할 수 있습니다. 이 검사는 해당 설치의 DB만 시작하고 Platform writer는 정지 상태로 유지합니다. 종료 시 기존 서비스 상태를 복구합니다. 실행 중이거나 소유권이 확인되지 않은 후보를 이 옵션으로 우회하지 마세요.

정지된 writer는 Docker 네트워크 endpoint에서 빠질 수 있습니다. 백업은 소유 DB endpoint를 필수로 확인하고, writer 실행 중에는 writer endpoint도 필수로 확인합니다. 정지된 writer endpoint의 부재만 허용하며 외부 endpoint는 계속 거부합니다.


## 로컬 앱 재시도 화면

앱의 「중단된 백업·복원 재시도」는 같은 코어를 호출합니다. 실제 macOS 네이티브 GUI 조작과 Windows/Podman·서명·공증은 별도 검증 조건입니다. 웹 미리보기는 실제 engine 권한이 없습니다.

1. 원래 실패·중단 작업이 있는 관리 공간을 선택하고 상태를 다시 확인합니다. 현재 공간의 작업 UUID만 선택할 수 있습니다. 새 작업이 이미 연결된 실패·중단 재시도는 반복 실행하지 않습니다.
2. 복원 재시도라면 「새 복원 공간 만들고 선택」으로 별도 공간을 준비한 뒤, 관리 공간 선택에서 원래 실패 작업의 공간으로 돌아옵니다. 재시도 화면의 목적지 선택에서 그 새 공간 ID를 지정합니다. 존재하는 공간이나 성공·실패 후보를 지우지 않습니다. 실제 빈 공간 여부는 코어가 검사합니다. override와 검증되지 않은 Windows 모드에서는 별도 등록 공간을 지정할 수 없습니다.
3. 비공개 외부 키 파일의 전체 경로와 신뢰할 유지보수 이미지 ID를 입력합니다. 복원에는 암호화 사본 경로와 새 loopback 포트도 필요합니다. 입력을 바꾸면 동의를 다시 확인해야 합니다. 키 내용은 입력하지 않으며 경로는 화면 임시 메모리에만 유지합니다. 관리 공간 선택 token이 바뀌면 입력과 결과를 초기화합니다.
4. 후보·원래 기록 보존과 별도 쓰기 중지·중단/새 설치 동의를 각각 확인하고 「새 작업으로 재시도」를 실행합니다. 새 작업이 시작되면 위의 「진행 중 백업·복원 취소」는 해당 새 작업을 대상으로 요청합니다. 복원 재시도 중에도 source 공간 선택을 유지하지만 취소 명령은 등록된 destination의 실제 자식 작업으로 연결됩니다. 요청 저장과 실제 정지 확인은 다릅니다. helper 확인이 끝나기 전의 원래 작업이나 이전 child를 임의로 취소하지 않습니다.
5. 결과와 새 작업 ID를 확인합니다. 복원 성공 후에는 표시된 목적지 공간을 직접 선택해 전시의 실제 응답을 확인합니다. 앱은 공간을 자동 전환하거나 원래 실패 기록을 완료로 바꾸지 않습니다. 백업 성공 후에도 원래 writer는 자동 재개하지 않습니다.
6. 실패·앱 종료 후에는 source의 새 작업 연결 기록을 확인합니다. 저장된 destination path SHA256을 현재 등록 공간의 경로와 비교해 공간 ID를 표시합니다. 매칭되지 않거나 접근 불가능하면 목적지를 추측하지 않습니다. 자식 ID가 있는 재시도는 그 공간에서 child 기록·helper·후보를 확인하세요. 앱 재실행 시 source→destination 취소 연결을 자동 재개하지 않습니다.

네이티브 요청은 `selectionToken`, 실패 job UUID와 등록 destination UUID를 사용합니다. 임의 destination path, raw command, 키 내용, 알 수 없는 필드를 받지 않습니다. source selection read guard와 profile operation lock은 전체 복원 재시도를 보호합니다. 다른 앱/CLI의 선택 변경, stale token, 미등록/같은 공간, symlink/권한 불일치는 거부합니다. destination별 원래 journal·freshness·helper·volume·runtime 검사는 CLI 코어가 계속 담당합니다.

취소 라우팅은 경로만으로 목적지 작업을 선택하지 않습니다. source/destination 작업 잠금 안에서 생성된 이번 재시도 parent 기록을 연결하고, 그 기록에 저장된 새 child UUID와 목적지 hash를 검증합니다. 아직 parent/child 연결 전이면 `BUSY`, 목적지의 현재 작업이 다른 UUID·종류이면 `CANCEL_TARGET_INVALID`로 거부하며 그 작업의 취소 상태를 쓰지 않습니다. 원래 공간 선택은 유지됩니다.

새 child UUID가 없는 중단은 [증거 기반 재시도 진단·복구 CLI](retry-recovery.md)를 사용합니다. 준비·예약과 후보가 일치한 경우에만 연결을 복구하거나 실제 생성 전 중단을 해제합니다. 증거 없는 이전 기록·불완전 후보·변경된 원본은 계속 차단합니다. 네이티브 진단 화면과 전체 backup/restore acceptance는 후속 조건입니다.
