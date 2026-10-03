# 암호화 사본에서 새 설치 복원 — 개발 CLI

`restore-backup`은 Manager producer가 만든 draft2 서비스 백업을 **비어 있는 새 설치 root**에 복원한다. 기존 설치·암호화 사본·외부 키를 덮어쓰지 않는다. 현재 macOS/Docker reference 구성의 개발 구현이다. Windows ACL/Podman, 앱의 복원 화면, 취소·재개, cold-engine 및 전체 frozen corpus qualification, 업데이트·rollback과 배포 서명은 별도 조건이다.

```sh
exhibitos-manager --root '<new canonical private absolute root>' restore-backup \
  'sha256:<trusted immutable local maintenance image ID>' \
  '<canonical mode0600 32-byte external key file>' \
  '<canonical mode0700 encrypted archive directory>' \
  '<unused loopback port above 1023>' --fresh-installation
exhibitos-manager --root '<same root>' restoration-status
exhibitos-manager --root '<same root>' status
```

이미지는 운영자가 검토한 로컬 유지보수 content ID여야 한다. 사본 인증은 키 소유를 확인하며 배포 서명이나 이미지 공급자를 인증하는 기능이 아니다. 사본의 보존된 Runtime/PostgreSQL content ID·크기·해시와 실제 import된 이미지가 일치해야 한다. Registry pull이나 임의 shell 입력은 사용하지 않는다. 키 내용·DB URL·비밀번호를 인수나 응답에 넣지 않는다. 루트는 operation.lock 외에 파일이 없어야 하며 기존 설치나 실패 후보를 지워 조건을 맞추지 않는다. 원본과 분리된 새로운 root를 선택한다.

## 실제 순서와 성공 조건

1. 공유 operation lock, 명시적 fresh 설치 동의, canonical private 경로·외부 key·파일 권한·중첩 금지·가용 공간·Docker/Compose·trusted image를 검사하고 새 포트를 예약한다.
2. 네트워크 없는 별도 helper가 전체 사본의 암호화 인증·닫힌 inventory를 검사한다. 새 private 작업 공간에서만 설정·이미지 후보를 추출한다.
3. 기존 bundle/installed/Compose hash·6-key 환경·현재 2-service/3-volume reference 구성을 검사한다. 지원하지 않는 환경·실행 명령·runtime-role 추가 파일은 조용히 누락하지 않고 거부한다. 현재 password URL 구분자는 거부하며 기존 generated credential은 보존한다.
4. 새 bundle/project ID와 새 loopback 포트만 부여한다. tenant/admin/PostgreSQL credential과 논리 DB·object identity는 보존한다. content ID로 참조하는 새 Compose와 private 설정을 만들고 두 image archive를 실제 load한 뒤 identity를 재검사한다.
5. 충돌 없는 새 labelled local volumes/network에 정지된 전시 container와 PostgreSQL을 만들고 DB만 시작한다. 드라이버·options·소유 label과 준비 상태를 검사한다. 이전 volume을 mount하거나 재사용하지 않는다.
6. writer가 시작되지 않은 새 DB/blob에 공개 서비스 복원 CLI를 실행해 DB/object inventory equality를 검사한다. 사전 인증과 실제 복원 manifest hash/backup ID가 같아야 한다. 서명 키와 blob을 reference Runtime UID1000에만 활성화한다.
7. 준비가 확인된 새 전시를 시작하고 실제 readiness/version을 기다린다. 그 후에만 private receipt와 completed job을 기록하고 `restored-and-running` 결과를 반환한다. 파일 인증만으로 DB 복원·실행 성공을 반환하지 않는다.

환경의 `EXHIBITOS_PORT`와 서비스 origin만 새 주소로 바뀐다. 기존 비밀번호나 tenant ID는 재발급하지 않는다. 현재 구현은 원래 artifact를 immutable image content ID로 바꿔 참조하므로 registry name/태그와 무관하게 보존된 이미지로 실행한다. 새 Compose는 현재 reference local profile로 제한하며 별도 custom 구성 지원은 후속 명세·검사가 필요하다.

## 실패·중단과 원본 보존

`restoration.json`은 id/state/stage/error/time만 저장한다. key bytes·원래 경로·비밀번호·raw engine 로그는 응답과 기록에 넣지 않는다. 완료 상태는 같은 ID의 receipt·manifest와 연결되어야 하며 단순 completed 문자열을 믿지 않는다. `restore-<UUID>/`와 private plaintext/이미지/실패 후보는 보존된다. 이 데이터는 Git/공유/웹 배포 밖에 둔다.

정상 오류는 exact-owned helper만 종료한다. 새 candidate container는 정확한 새 bundle/project label을 재검사한 뒤 정지시키며 volumes·DB·사본은 삭제하지 않는다. 호스트 중단 후 running job은 interrupted가 되며 일반 install/start/restart/retry가 RESTORE_RECOVERY_REQUIRED로 차단된다. 정확한 label과 private 후보를 조사하고 새로운 빈 root로 다시 복원한다. 중단 후 자동으로 helper를 지우거나 사본을 성공 처리하거나 부분 DB 위에 재실행하지 않는다. 명시적 정지는 허용한다.

호스트 filesystem snapshot 또는 관리자와의 동시 파일 변경 격리를 보장하지 않는다. 실제 운영 복구는 외부 writer를 정지하고 별도 검증된 복원 지점·보관 키·용량·권리·정책을 갖춰야 한다. 최초 개발 사용에서도 source archive/key와 이전 설치를 보존한다.

## 재현 검사

먼저 [생성 검사](backup-creation.md)로 실제 합성 Manager 설치와 encrypted archive, fresh manual restore witness를 만든다. 이전 데이터 대신 새 labelled synthetic fixture를 사용한다.

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --locked -p exhibitos-lifecycle --bin exhibitos-manager
python3 scripts/test-backup-restoration.py \
  --fixture '<retained synthetic backup-creation fixture>' \
  --manager "$PWD/target/debug/exhibitos-manager"
```

시험은 외부 새 경로에 사본을 복사하고 기존 source Manager 경로를 잠시 unavailable로 만든 후 반드시 원래 경로로 복구한다. 잘못된 키/변조/alias/기존 대상/lock을 거부하고 원래 계정의 실제 HTTP login, DB witness/blob/signing key, 실제 정지·시작과 원본 ciphertext/key/service states 보존을 확인한다. 새 private 후보·volume은 보존하고 정확한 새 test container만 정지한다. 같은 엔진의 image cache를 유지하므로 archive load 경로 검증과 cold-engine qualification을 구분한다. 실제 검사 결과는 기록 후에만 통과로 보고한다.

## 2026-10-03 로컬 실제 검사

Rust workspace 51개와 엄격한 all-target Clippy, CLI 빌드가 통과했다. 같은 Docker 엔진의 합성 사본으로 실제 검사 7개 그룹을 통과했다: 사전 안전 조건, 잘못된 키, 변조, 원본 경로 unavailable 상태에서 새 설치 활성화, 기존 관리자 HTTP 로그인과 DB/blob/서명 키 동일성, 실제 stop/start, 원본 ciphertext/key/container 상태 보존. 실패했던 후보는 보존했고 원본 경로를 복구했다.

초기 검사는 실패한 job 조회의 CLI 종료 코드와 Compose의 기본 null 처리, 복원된 파일의 소유권 변경 후 chmod 순서 문제를 발견했다. 수정 후 새로운 빈 대상에서 전체 실제 검사를 다시 통과했다. 이 결과는 native GUI·Windows·cold-engine·전체 frozen corpus·취소/재개·OEX·서명된 update/rollback 검증을 대신하지 않는다. 프런트엔드 변경은 없다.
