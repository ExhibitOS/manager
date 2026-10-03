# 암호화 백업 인증 검증 — 개발 CLI

`LifecycleService::verify_backup`와 `exhibitos-manager verify-backup`은 Platform의
공개 service-backup CLI를 별도 유지보수 이미지에서 실행한다. 실제 암호화
사본의 manifest와 각 파일의 인증·크기·해시·닫힌 inventory를 검사한다.
백업 생성, DB/blob 복원, 서비스 활성화, 자동 업데이트·rollback과 Manager UI는
이 경로가 수행하지 않는다. 인증 검증은 독립 새 환경 복원 검증을 대체하지 않는다.

## 준비와 실행

신뢰할 운영자가 설치된 Manager root, 검사한 로컬 유지보수 이미지의 `sha256:`
content ID, 별도 mode0600/32byte key 파일과 mode0700 암호화 사본 디렉터리를
준비한다. 태그·registry pull·임의 shell을 입력받지 않는다. 이미지 선택 권한은
운영자에게 있으며 이 개발 경로가 배포 서명을 검증했다고 해석하지 않는다.
기존 bundle/installed metadata와 pinned engine을 검사한 뒤 해당 엔진만 사용한다.
이미지를 다른 엔진에서 만들었다면 같은 content ID의 승인된 archive를 먼저
정상적으로 import해야 하며 조용히 다른 엔진으로 전환하지 않는다.

```sh
cargo build --locked -p exhibitos-lifecycle --bin exhibitos-manager
target/debug/exhibitos-manager --root "$MANAGER_ROOT" verify-backup \
  "$MAINTENANCE_IMAGE_ID" "$BACKUP_KEY_FILE" "$ENCRYPTED_BACKUP_DIRECTORY"
```

변수는 경로와 content ID이며 key의 내용이나 DB URL/password를 명령 인수에
넣지 않는다. root/key/source는 canonical absolute path여야 하고 symlink나 mount
구문을 바꿀 수 있는 comma/control 문자는 거부한다. source와 root는 서로
중첩할 수 없고 key는 source 밖에 둔다. 다른 접근 권한·파일시스템·OS 설정은
이 CLI가 자동 수정하지 않는다. Windows는 ACL·mount 검증이 완료될 때까지
명시적으로 `BACKUP_PLATFORM_UNVERIFIED`로 거부한다. Linux/Podman 실행은
구현 후보이나 실제 qualification은 아직 없으며 macOS Docker 결과와 구분한다.

## 원본 보존과 비공개 작업 공간

기존 lifecycle과 같은 cross-process operation lock으로 중복 변경을 막는다.
새 `backup-verification-<UUID>` 디렉터리만 root 안에 만들고 원본 archive/key는
read-only mount한다. 네트워크 없음, read-only container root, cap-drop ALL,
no-new-privileges, 제한 tmpfs, 실제 private 파일 소유 UID/GID로 실행한다.
Docker socket이나 임의 host root를 mount하지 않는다. DB에는 연결하지 않는다.

성공 결과에는 operation ID, verified 파일 수, image ID, 인증된 plaintext
manifest SHA-256과 시각만 들어간다. root의 새 작업 공간에는 mode0600
receipt.json과 **민감한 복호화 데이터**인 plaintext 디렉터리가 남는다. 이
작업 공간은 공유·Git·웹 배포에 넣지 않는다. receipt가 현재 원본 DB inventory나
실제 복원 성공, 보관 key 복구, 현재 권리·서비스 health를 증명하지는 않는다.
후속 update preflight에서 이 결과만으로 restore 검증을 true로 만들지 않는다.

실패하면 새 후보에 failed.json을 남기고 기존 key/사본/서비스 데이터는 보존한다.
엔진 stderr와 inventory·경로·key 내용은 native 응답에 포함하지 않는다. 기본
실행 제한은 1시간이고 오류 시 exact owned container label을 확인한 뒤 해당
검사 컨테이너만 정리한다. 실패 작업 공간과 부분 plaintext는 삭제하지 않는다.
호스트 SIGKILL/전원 중단 시 정리가 보장되지 않으므로 root의 후보 UUID와
`com.exhibitos.verification=<같은 UUID>` label이 일치하는 검사 컨테이너 및 private
작업 공간을 운영자가 조사해야 한다. 다른 컨테이너·기존 백업을 자동 삭제하지 않는다.

## 실제 검사 범위

Rust workspace core21, 실제 Unix permission1, update-safety13와 native wrapper1
검사 및 strict clippy PASS. 별도 synthetic 설치 metadata root에서 실제 Docker29.8
이미지로 8개 검사를 실행했다: 정상 encrypted archive 인증/manifest receipt
해시 연결, wrong key, corrupted authenticated archive, mutable image tag 거부,
public key mode 거부, key symlink 거부, 실제 process lock, 원본 archive/key
byte equality. 저장된 private 실패 후보와 plaintext는 보존됐다. 이 검사는
Manager native GUI·Windows·생성/복원·signed updates·cancel UI·장애 후 자동
재개 acceptance가 아니다.

재현은 Platform의 actual synthetic service-backup 검사에서 생성된 사본·key와
인증된 expected manifest, 의도적으로 corrupted fixture를 준비하고 **새**
Manager root에 설치 metadata/bundle을 구성한다. 원래 사용자 사본을 corrupted
fixture로 사용하지 않는다. Python3/Unix 및 actual engine이 필요하다.

```sh
python3 scripts/test-backup-verification.py \
  --manager target/debug/exhibitos-manager --root "$SYNTHETIC_MANAGER_ROOT" \
  --image "$MAINTENANCE_IMAGE_ID" --key "$SYNTHETIC_KEY_FILE" \
  --source "$SYNTHETIC_ARCHIVE" --expected-manifest "$VERIFIED_MANIFEST" \
  --corrupted-source "$SYNTHETIC_CORRUPTED_ARCHIVE"
```

시험은 새 root에 별도 잘못된 key와 alias/후보를 추가하며 원본 key/사본은
바꾸지 않는다. 재실행은 새 root로 수행하고 이전 작업 공간은 보존한다.
