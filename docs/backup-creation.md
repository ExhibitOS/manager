# 설치된 전시의 암호화 백업 생성 — 개발 CLI

`create-backup`은 현재 Docker 기반 Manager 설치의 DB·작품 bytes·서명 설정·설치
파일과 두 실행 image archive를 암호화한다. 백업을 다시 복호화해 인증하고,
호스트로 복사한 ciphertext의 전체 파일 목록·바이트 수·해시·private 권한을
확인한 뒤에만 완료 receipt를 기록한다. 로컬 앱 창에는 같은 producer를 호출하는 생성 화면과 durable 작업 기록을 연결했다.
**Manager 복원 활성화·실제 네이티브 GUI·Windows·Podman·OEX·signed
update/rollback은 아직 별도 구현·검증이 필요하다.**

## 준비와 실행

1. 현재 설치 bundle·image 출처와 로컬 immutable 유지보수 image ID를 운영자가
   검증한다. 태그나 자동 image pull은 허용하지 않는다. Runtime과 유지보수 image가
   같다고 가정하지 않으며 동일한 public storage/schema 계약을 제공해야 한다.
2. 다른 앱·worker·스크립트의 DB·작품·설정 변경을 중지한다. Manager operation
   lock은 다른 도구의 writer를 막지 못한다. 다른 container나 외부 직접 DB 접근도
   확인한다. 승인 flag만으로 실제 외부 quiescence가 증명되는 것은 아니다.
3. canonical absolute private 경로의 별도 32-byte key를 준비한다. key는 0600의
   regular single-link 파일이어야 하고 Manager root 밖에 둔다. 분실하면 복원할 수
   없으며 key bytes를 argv·Git·로그나 채팅에 넣지 않는다.
4. 아래 명령을 실행한다. 성공·실패 모두 전시 Platform은 정지 상태로 남을 수
   있다. 확인 후 일반 `start` 명령으로 명시적으로 재개한다. DB volume은 정지나
   백업 생성 과정에서 삭제하지 않는다.

```sh
exhibitos-manager --root '<canonical private absolute root>' create-backup \
  'sha256:<verified maintenance image ID>' '<canonical private key file>' \
  --external-writers-quiesced
exhibitos-manager --root '<canonical private absolute root>' backup-jobs
exhibitos-manager --root '<canonical private absolute root>' start
```

현재 producer는 `database`·`platform` 두 서비스, PostgreSQL18의
`/var/lib/postgresql`, `/data/blobs`·`/data/config` named local volumes와 해당
project의 default bridge network를 지원한다. bundle/Compose 해시·설치 metadata·
volume/network/container ownership·pinned image identity·실제 container 환경과
보존할 환경이 일치해야 한다. 외부 volume·다른 network·추가 서비스/설정은
자동 추측하지 않는다. [설치 설정 보존](installation-backup.md)의 6-key 형식을
유지하며 credentials를 새로 생성하지 않는다.

원래 Runtime·PostgreSQL images를 새 private 파일로 저장하고 content ID·원래
registry pin·새 archive 크기/해시를 authenticated configuration에 포함한다.
`docker save` 사본과 원래 배포 archive가 바이트 동일하다고 가정하지 않는다.
복구자는 이 inventory를 검증하고 필요한 engine별 신뢰/호환성 검사를 수행해야
하며 저장된 registry pin을 새 engine이 자동으로 보유한다고 가정하지 않는다.

## 격리와 결과

읽기 전용 root filesystem과 DB를 제외한 source blob/configuration volumes의
읽기 전용 mount를 사용한다. 유지보수 container는 해당 project network에서만
DB에 연결하고 privileged·host network·Docker socket mount를 사용하지 않는다.
컨테이너/host의 서로 다른 파일 UID를 읽기 위해 UID0와 `DAC_OVERRIDE`만
허용하며 나머지 capability는 모두 drop한다. Source mount는 이 권한이 있어도
읽기 전용이다. 1536MiB memory·128 pids·256MiB tmpfs 및 2GiB dump/image
bounds를 적용한다. 이는 무제한 대용량 backup 지원이나 disk quota 보장이 아니다.
image 사본 저장 전/후 여유 공간을 검사하지만 DB/blob 전체 추가 사용량은
데이터 크기에 따라 달라지므로 운영 disk capacity를 별도로 확인한다.

`backup-creation-<job id>/`의 0700 workspace에는 안전한 job.json,
image archives와 완료된 encrypted `archive/`, receipt.json이 남는다. Key는
보관하지 않는다. 영속 job stage는 preflight, saving-images, pausing-writers,
encrypting-and-authenticating, copying-authenticated-archive, complete로 진행한다.
Receipt는 id/backupId/operation/files/image/authenticatedManifestSha256/
writersPaused/at만 반환한다. 완료 상태는 restore qualification을 의미하지 않는다.

새 `exhibitos-backup-work-<job id>` engine volume에는 암호화 사본과 인증 검사의
private plaintext가 보존된다. 이 volume·설정·key·host image archives·DB/blob와
실패 후보는 Git backup 범위 밖이다. Engine 접근 권한 보유자는 이 자료를 볼 수
있으므로 평문을 일반 공유·웹 export에 넣지 않는다. 자동 retention/prune는 없다.

실패한 helper는 정확한 생성 label을 확인해 해당 container만 종료한다. 해당
volume과 host 후보는 보존한다. 앱/host 강제 종료 후 running job은 interrupted로
표시하고 성공 receipt를 추측하지 않는다. 강제 종료는 CLI의 helper cleanup까지
보장하지 않으므로 job id와 `com.exhibitos.backup` label로 남은 helper를 확인하고
원본 writer가 계속 중지되어 있는지 확인한다. 새 backup은 이전 job의 정확한
이름·label을 가진 helper가 아직 running이면 BACKUP_ORPHAN_PENDING으로 거부한다. 실제 native cancel/retry·orphan
reconciliation UI는 후속 작업이며, 자동 재개나 오래된 사본 삭제는 없다.

## 실제 엔진 검사

검증된 local images가 이미 존재하는 환경에서 실행한다. 새 labelled 합성 설치만
만들며 기존 사용자 root·volume을 대상으로 삼지 않는다.

```sh
cargo build --locked --bin exhibitos-manager
python3 scripts/test-backup-creation.py \
  --manager "$PWD/target/debug/exhibitos-manager" \
  --runtime-image 'sha256:<verified Runtime image ID>' \
  --maintenance-image 'sha256:<verified maintenance image ID>' \
  --postgres-image 'postgres:18.6@sha256:<verified registry digest>'
```

기본 fixture 부모는 `/private/tmp`다. 임시 파일 정리에 영향을 받지 않고 후속
검사를 이어가려면 `--workspace-parent <existing canonical absolute directory>`를
추가한다. 지정 폴더는 현재 사용자 소유이고 group/other 접근 권한이 없어야
한다(예:0700). 검사는 그 아래 고유한 새 폴더를 만들며 기존 fixture를
덮어쓰거나 삭제하지 않는다. 암호화 사본·외부 키·복호화 자료는 Git 밖에
보존하고, 이 로컬 검사용 자료를 독립 원격 백업으로 취급하지 않는다.

검사는 actual Manager install/start, 사전 권한·ack·lock 거부, backup creation,
원래 설치 재시작, source path/DB unavailable 및 별도 empty PostgreSQL/blob
복원과 원래 credentials·signing key·양쪽 image archive의 일치를 확인한다.
같은 Docker engine의 검사이며 cold engine image import/Manager restore
activation/native GUI/Windows/Podman/production 복원을 증명하지 않는다. 새
합성 containers는 정확한 label로 정지하고 volumes·후보·보고서는 보존한다.

위 검사의 보존된 fixture 경로에 대해 추가 orphan 검사를 수행한다. 기존 실제
작업 volume의 ownership과 별도로 새 합성 helper를 만든 뒤, running job의
interrupted 복구 및 새 backup 차단을 확인하고 그 helper만 정확한 label로
제거한다. 기존 volume/사본은 유지한다.

```sh
python3 scripts/test-backup-creation-recovery.py \
  --manager "$PWD/target/debug/exhibitos-manager" \
  --fixture '<printed absolute synthetic fixture directory>'
```


## 로컬 Manager 생성 화면

전시 설치와 실행 도구 확인 후 ‘설치된 전시 백업’에서 외부 key 파일의 실제
전체 경로와 신뢰할 로컬 immutable 유지보수 image ID를 준비한다. Key bytes는
입력하지 않는다. 입력은 화면 메모리에만 유지하고 local/session storage나
로그에 저장하지 않는다. 키 생성·파일 picker·credential store 자동 등록은
아직 제공하지 않으므로 기존의 별도 비공개 32-byte key(0600)를 준비한다.

다른 앱·worker·스크립트의 DB/blob/configuration writer가 실제로 중지되었는지
운영자가 확인하고, 전시가 중단되며 수동 재개해야 한다는 별도 체크박스까지
동의해야 ‘전시를 멈추고 백업 생성’을 실행할 수 있다. `manager_create_backup`
네이티브 command도 두 acknowledgement를 boolean으로 검사하고 unknown
fields를 거부한 후 기존 producer의 lock·image·path·key mode·ownership·writer
검사를 실행한다. 로컬 main window origin와 명시적 Tauri capability가 필요하며
웹 preview나 remote page는 실행 권한이 없다. UI 확인만으로 외부 quiescence가
증명되는 것은 아니다.

작업 중에는 중복 생성·검증·설치·시작·재시작·재시도를 막는다. 예상 진행률을
만들지 않고 indeterminate 상태를 표시한다. 백업 완료 receipt의 정확한 필드,
operation, image binding, 두 UUID, 인증 hash, 파일 count와 `writersPaused:true`가
일치해야 성공을 표시한다. ‘created’만 있는 응답이나 다른 image/restore 응답은
완료로 취급하지 않는다. 성공·실패 이후에는 두 동의를 해제하므로 새 생성 시
다시 확인해야 한다. 오류는 alert에 키보드 초점을 옮기고 사본/원본 보존과
남은 helper 조사를 안내하며, raw engine exceptions는 표시하지 않는다.

새 암호화 사본은 설치 root의 `backup-creation-<job id>/archive/`에 보관한다.
이 화면은 외부 매체로 자동 복사하거나 key를 보관하지 않으며, 동일한 디스크의
사본만으로 디스크 고장 복구가 보장되지 않는다. 원본 credentials/서명 설정과
민감한 인증 검사의 plaintext/work volume은 기존 producer 정책대로 보존한다.

‘백업 생성 기록’은 `manager_backup_jobs`를 통해 실제 영속 job을 읽는다. 창을
다시 열어도 interrupted/failed가 completed로 바뀌었다고 표시하지 않는다.
기록을 읽지 못하면 새 생성·검증·lifecycle mutation을 중지하되 실행 도구 다시
확인으로 정상 기록을 다시 읽을 수 있다. 진행 중인 저장 작업이 있으면 다른
변경을 막고 polling으로 상태를 확인한다. 취소/자동 retry/orphan reconciliation은
아직 제공하지 않으며, 중단 ID와 정확한 label을 확인해 남은 helper를 조사한 뒤
새 작업을 명시적으로 생성한다. 실패 후보나 volume을 자동으로 삭제하지 않는다.

브라우저의 synthetic IPC 검사는 이 입력·dispatch·presentation 경계를 검사하며
actual native GUI/engine/crypto/restore acceptance를 대신하지 않는다. 기존 실제
CLI producer와 새 PostgreSQL/blob restore 증거는 별도로 보존한다.


### 생성 화면의 검증 기록 — 2026-10-03

최종 frontend 파일 18개의 SHA-256을 비교한 별도 임시 경로에서 `npm ci`,
`npm run check`(typecheck/lint/단위8/production build), `npm run test:browser`
30개가 통과했다. 정상·실패·malformed/wrong-image 응답, 두 동의/키 경로,
중복 submit, mutation·검증 제외, stale readiness 제거(기존 in-flight status 응답의 세대 거부 포함), 화면을 다시 연 뒤
running/interrupted 기록, malformed history의 fail-closed와 다시 확인, 동의
초기화, stale success 제거, session storage 없음, 오류 초점과320/640/1120px
레이아웃·44px form targets를 검사했다. Screenshot의 desktop 정상 및320px
실패 화면도 직접 확인했고, 안내 문구 대비와 checkbox target을 개선했다.

Rust workspace46(core28/Unixpermission1/update13/native wrapper4), locked/offline
검사와 strict all-target Clippy, rustfmt, diff whitespace 검사도 통과했다.
새 네이티브 JSON request 시험에 필요한 기존 serde_json1.0.151을 test-only로
명시했으며 lockfile에는 이 dependency edge만 추가했다. 최초 시험의 누락된
직접 dependency 오류는 수정 후 재검증했다. 저장 공간 절약을 위해 검증은
incremental/debug info 없이 수행했고, 로컬 SDK의 stripping helper 문제는
strip=none으로 피했다. 보안 설정이나 기존 SDK 파일은 변경하지 않았다.

문서 폴더의 FileProvider가 지연시킨 원래 workspace lint는 exact final-source
독립 검사 완료 후 해당 owned process만 종료했다. 이를 통과로 바꾸어 기록하지
않는다. Synthetic IPC와 native wrapper 컴파일/단위 검사는 실제 macOS 앱의
입력→생성→receipt 조작이나 VoiceOver·Windows·새 engine 복원을 증명하지 않는다.
실제 Mac은 현재 잠금 상태라 GUI 조작 검사는 남아 있다. 실제 CLI11항목 사본
생성 및 원본 unavailable PostgreSQL/blob/config/images 복원 증거는 앞선
producer 검사와 별도로 유지하며 이번 UI 변경에서 반복 실행하지 않았다.

생성 직후 새 status 응답이 지연되어도 설치·시작·재시도 버튼을 활성화하지 않는다. 완료 응답이 유실되거나 malformed이면 실패를 단정하지 않고 완료 여부를 확인하지 못했다고 안내한다. 소스 변경으로 오래된 frontend snapshot을 대상으로 하던 native build 두 번은 해당 owned Cargo만 SIGINT로 종료하고 dependencies/logs를 보존했다. 최종 source와 같은 snapshot/config로 다시 패키징하며 취소한 시도를 통과로 기록하지 않는다.

최종 macOS 개발 앱 패키징은 같은18개 frontend source hash를 확인한 production snapshot을 사용해 통과했다. 임시 Tauri build override로 검증된 `dist`를 embed하고 지연된 workspace beforeBuildCommand만 건너뛰었으며 override는 source/config에 저장하지 않았다. Mach-O arm64 실행 파일은11,868,792bytes, SHA-256 `49a8ff9a677091539c55eae109be211615196c69ebba8816dba21d316edf2777`이다. Linker ad-hoc signature만 존재하고 TeamIdentifier·sealed bundle resources는 없으므로 배포 서명·공증·실제 GUI acceptance가 아니다.

## 중단된 helper와 전시 재개 보호

백업 helper가 남아 있는 상태에서 전시 writer를 시작하면 보존 작업과 새 쓰기가 겹칠 수 있다. Manager의 설치·시작·재시작·해당 작업 재시도는 operation lock 안에서 기존 백업 job과 실행 도구의 실제 helper 상태를 검사한다. 완료/중단/실패 기록만으로 helper가 끝났다고 추정하지 않는다. 작업 ID와 이름·label이 일치하는 helper가 실행 중이면 BACKUP_ORPHAN_PENDING으로 실패하며 전시와 설치 설정을 변경하지 않는다.

helper는 label 및 이름 범위의 합집합으로 찾는다. 원래 소유 label은 남았으나 이름이 변경되었거나, 원래 이름에 다른 label이 붙은 경우 OWNERSHIP_CONFLICT로 실패한다. 실제 Running 응답이 bool이 아니면 ENGINE_OUTPUT_INVALID로 차단한다. 다른 설치의 helper는 작업 ID가 관계없으면 영향을 주지 않는다. 정지는 writer를 계속 정지시키는 비상 경로로 허용하며 자동 helper 종료/데이터 삭제/사본 성공 판정은 하지 않는다.

이 보호는 보존된 작업 기록과 실제 실행 도구 응답에 의존한다. 실행 도구 관리자가 job/label/name 모두를 임의 변경하는 행위를 격리하는 sandbox는 아니다. 취소·정확한 orphan 정리 UI와 전체 복원 활성화는 여전히 후속 구현이다.

실패·중단 후 남은 보조 실행은 [helper 확인과 정지](helper-reconciliation.md)에서 명시적으로 확인할 수 있다. 원래 작업을 성공 처리하거나 후보 데이터를 삭제하지 않습니다. 실행 중 백업·새 설치 복원 취소는 [현재 취소 절차](maintenance-cancellation.md)를 사용합니다.

진행 중 백업 생성·새 설치 복원은 [취소 요청과 실제 정지 확인](maintenance-cancellation.md)을 사용합니다. 요청 저장·앱 종료·helper 확인을 취소 성공으로 혼동하지 마세요.
