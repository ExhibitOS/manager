# 실패·중단 작업의 helper 확인과 정지

앱의 **중단된 작업의 helper 확인**은 현재 관리 공간에 저장된 실패·중단 백업 생성 또는 새 설치 복원 작업만 대상으로 한다. 작업을 선택하고 후보 보존 동의를 확인한 뒤 **해당 helper 확인·정지**를 누른다. 실행 중인 작업 취소와 자동 재개는 별도 구현 항목이다.

1. 실행 중인 앱 작업이 끝나거나 중단된 것을 먼저 확인한다. 같은 관리 공간의 operation lock이 잡혀 있으면 `BUSY`로 거부한다.
2. 소유자가 읽을 수 있는 비공개 작업 기록에서 UUID·작업 종류·실패/중단 상태를 확인한다. 완료·진행 중 작업이나 임의 container ID/명령은 받을 수 없다.
3. Docker에서 작업의 이름과 label 범위를 각각 조회한다. 두 범위가 같은 단일 helper를 가리켜야 한다. 이름 변경·label 충돌·중복 대상·잘못된 engine 응답은 자동 정지하지 않는다.
4. 실제 전체 container ID·이름·label·image identity·비특권/읽기 전용 root·쓰기 mount 범위를 확인하고 정지 직전에 다시 검사한다. 백업 helper는 해당 작업의 `/work` volume, 복원 helper는 해당 작업의 `/work` bind와 검증된 새 설치의 저장 volume만 쓰기 대상으로 허용한다.
5. 검증된 image가 선언한 `/var/lib/postgresql` 익명 volume은 local driver와 정확한 전체 이름을 확인한다. `--rm` helper의 경우 정지 전에 같은 immutable image로 **실행하지 않는 보존 컨테이너**를 만들고, 해당 volume만 읽기 전용·`volume-nocopy`로 연결한다. 보존 컨테이너의 ID·이름·label·image·정지 상태·mount를 확인한 뒤에만 원래 helper를 정지한다. 선언되지 않은 쓰기 volume이나 다른 image volume 경로는 거부한다.
6. 정확한 ID로 정지한 뒤 다시 조회한다. 확인된 정지 또는 두 조회 범위에서 확인된 부재만 결과로 반환한다. 조회 오류·달라진 소유권·아직 실행 중 상태는 완료로 표시하지 않는다.

**정지 확인은 백업·복원 성공이 아니다.** 원래 작업 상태·사본·키·DB·volume은 바꾸거나 삭제하지 않는다. 서버를 자동 재개하거나 실패 후보 위에 재실행하지 않는다. 복원을 다시 시도하려면 [새 관리 공간](installation-selection.md)을 만들고 보존한 사본으로 복원한다. 백업을 다시 만들 때도 외부 writer 중지·전시 중단 동의를 새로 확인한다. 실제 시작 때의 helper 검사 경계는 유지한다.

container 삭제 명령이나 volume 삭제 명령은 보내지 않는다. 기존 `--rm` 설정으로 실행한 복원 helper는 Docker가 종료 후 자동 제거할 수 있다. bind 작업 공간과 volume·원본 데이터는 보존한다. [Docker의 자동 제거 규칙](https://docs.docker.com/reference/cli/docker/container/run/)은 익명 volume도 제거할 수 있으므로 보존 컨테이너로 연결을 유지하고 정지 후 volume 존재를 다시 확인한다. 보존 컨테이너는 시작·자동 삭제하지 않는다. 컨테이너가 사용하는 volume은 [Docker volume 삭제 규칙](https://docs.docker.com/reference/cli/docker/volume/rm/)에 따라 보존된다. 호스트 관리자가 수동으로 보존 컨테이너를 삭제하거나 storage를 훼손하는 경우까지 방지하지는 않는다. 호스트 관리자와 동시에 이름·label·mount를 조작하는 악의적 변경의 완전한 격리를 보장하지 않으며, 이 기능을 임의 운영 container를 정지하는 도구로 사용하지 않는다.

## CLI와 기록

```sh
exhibitos-manager --root '<current canonical private root>' reconcile-helper \
  backup '<failed backup job UUID>' --preserve-candidates
exhibitos-manager --root '<current canonical private root>' reconcile-helper \
  restoration '<failed restoration job UUID>' --preserve-candidates
exhibitos-manager --root '<same root>' helper-reconciliations
```

`helper-reconciliation-<UUID>.json`은 비공개 root에 0600으로 저장한다. 대상 작업 ID·종류·확인 상태·오류 코드·시간·helper 상태만 기록하며 key bytes·원본 경로·credentials·raw inspect/log는 기록하지 않는다. `helper-retention-<확인 작업 UUID>.json`도 0600으로 보존하며 보존 컨테이너 ID·image ID·대상 작업·volume 이름·생성 시각만 연결한다. 생성 후 기록 중단 시에도 보존 컨테이너를 자동 삭제하지 않는다. 원래 백업·복원 journal은 덮어쓰지 않는다. 확인 중 앱이 중단되면 operation lock 해제 후 기록은 `interrupted`로 바뀌며 정지 확인으로 처리하지 않는다. 다시 확인해 실제 engine 상태를 읽는다. 잘못된 소유권은 고치거나 무시하지 않고 비공개 원본과 진단 기록을 보존한다.

기록의 `completed`는 **helper 확인 작업의 완료**이며 원래 백업·복원의 완료가 아니다. `helperState`는 `absent` 또는 `stopped`다. 영수증의 `dataPreserved: true`, `writersResumed: false`를 검사한다. native frontend는 선택 토큰과 대상 작업 ID·종류를 검사하고 입력 동의를 완료·오류 후 해제한다. 로컬 main 창에서만 호출할 수 있다.

감사 기록과 private candidates는 Git 밖이다. 이 메타데이터는 암호화된 DB/blob/설정 백업 또는 전체 복원 지점을 대체하지 않는다. 기존 사본·실패 후보의 자동 삭제나 보존 만료는 없다.

## 재현 검사와 제한

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --locked -p exhibitos-lifecycle --bin exhibitos-manager
python3 scripts/test-helper-reconciliation.py \
  --manager '<built native CLI>' \
  --image 'sha256:<reviewed local maintenance content ID>'
```

도구는 새로운 합성 helper·private roots·work volumes만 만들며 실제 정지, 이름/label·중복 충돌 거부, 활성 lock 거부, 원래 journal·work bytes·이전 container 상태 보존과 감사 기록 재조회/중단 복구를 검사한다. 자동 제거 helper의 익명 volume에 합성 witness를 쓰고 정지 후 동일 bytes, 실행하지 않은 보존 컨테이너와 0600 연결 기록을 확인한다. 정지된 테스트 candidates·보존 컨테이너·volume은 보존한다. 결과는 실행 후에만 통과로 기록한다. unit의 합성 engine 응답, browser의 합성 IPC, 실제 Docker 검사와 네이티브 GUI 검증을 구분한다.

Windows/Podman·실제 native GUI·실행 중 취소·서명된 update/rollback·cold-engine·전체 frozen corpus·OEX 검증은 이 기능으로 완료되지 않는다. 호스트 잠금 해제와 iCloud 설정 변경은 사용자의 별도 진행 의사를 따른다.

새 복원 helper는 `restoration-aux-<작업 UUID>.json`으로 연결한 작업별 named auxiliary volume을 사용합니다. 정확한 작업 이름·local driver·빈 options와 `com.exhibitos.restoration` 소유 label을 확인하고 정지 후 volume 존재를 재확인합니다. named volume에는 익명 volume용 보존 anchor를 추가하지 않습니다. 이전에 실행한 anonymous-volume helper의 anchor 보존 방식은 그대로 지원합니다.

macOS Docker Desktop이 `/work` bind를 VM 경로 `/host_mnt<host path>`로 보고할 때에는, engine info의 `OperatingSystem: Docker Desktop`·`OSType: linux`, 정확한 전체 VM 경로와 `HostConfig.Mounts`의 정확한 host 또는 동일 VM source·bind type·쓰기 설정·단일 `/work` 항목이 모두 일치해야 합니다. basename·임의 prefix 제거·`..` 경로·중복 target을 허용하지 않습니다. 다른 운영체제에는 이 매핑 예외를 적용하지 않습니다.
