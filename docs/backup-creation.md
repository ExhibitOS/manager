# 설치된 전시의 암호화 백업 생성 — 개발 CLI

`create-backup`은 현재 Docker 기반 Manager 설치의 DB·작품 bytes·서명 설정·설치
파일과 두 실행 image archive를 암호화한다. 백업을 다시 복호화해 인증하고,
호스트로 복사한 ciphertext의 전체 파일 목록·바이트 수·해시·private 권한을
확인한 뒤에만 완료 receipt를 기록한다. **Manager 복원 활성화·네이티브 생성 UI·
Windows·Podman·OEX·signed update/rollback은 아직 별도 구현·검증이 필요하다.**

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
