# 설치 설정 보존 — 개발 CLI

`prepare-installation-backup`은 암호화 서비스 백업에 포함할 기존 설치 설정을
새 비공개 작업 공간에 복사한다. **이 결과는 암호화 백업이나 DB·작품·이미지의
복원 지점이 아니다.** 전체 Manager 백업 생성·새 설치 복원·UI는 후속 연결 작업이다.

```sh
exhibitos-manager --root '<canonical private absolute root>' prepare-installation-backup
```

실행은 기존 operation lock을 공유한다. 현재 설치 명세와 installed.json이 같고,
Compose hash·엔진 선택·기존 installer의 6-key runtime.env 형식이 맞아야 한다.
필드가 추가되거나 구성이 바뀌면 자동으로 잘라내거나 비밀번호를 새로 만들지 않고
거부한다. 기존 credentials를 그대로 보존한다. 현재 Unix 구현만 허용하며 Windows
파일 권한/ACL와 실제 복구 지원 검증은 별도다.

파일 descriptor는 O_NOFOLLOW/O_NONBLOCK, bounded read, 단일 hard-link·파일
ownership·권한 검사와 읽기 전후 inode/수정 상태를 확인한다. manifest/Compose/
installed/engine/runtime.env 5개만 복사하며 임의 파일 선택이나 credential 출력은
제공하지 않는다. 복사 뒤 원본을 다시 검사한 후에만 안전한 receipt를 기록한다.
외부 사용자가 동시에 파일을 수정하는 상황까지 원자적 filesystem snapshot으로
주장하지 않는다. 운영 연결에서는 외부 설정 writer도 중지해야 한다.

`backup-preparation-<receipt.id>/`는 0700이며 파일은 0600이다.
`configuration-files.json`은 논리 이름→작업 공간 파일 경로의 private JSON이다.
Platform service-backup CLI의 `BACKUP_CONFIGURATION_FILES`에 해당 JSON을
비공개 environment에서 전달해 이 5개를 암호화 configuration으로 포함한다.
경로/내용을 argv·Git·공개 로그·웹 다운로드에 넣지 않는다. `inventory.json`의
크기/hash와 receipt의 inventorySha256으로 준비된 파일을 확인한다. Receipt는
id/operation/files/hash/time만 포함하며 비밀번호·원래 경로를 반환하지 않는다.

새 복구에서는 엔진 정보가 기록되어 있어도 archived engine 설정을 맹목적으로
활성화하면 안 된다. 대상 엔진을 재검사하고 bundle/Compose/image provenance와
volume ownership·포트·schema compatibility를 독립적으로 확인해야 한다.
이 묶음에는 volume 속의 서명 키·DB·작품·OCI image archives·외부 key가 없다.
그 데이터와 이 설치 설정을 일관된 서비스 백업에 함께 담고 fresh target의 실제
복원을 검증한 뒤에만 복원 지점으로 승인한다.

실패 중 새 작업 공간이 생기면 failed.json과 private 후보를 보존한다. 이전
작업 공간·설정·데이터를 자동 삭제하지 않는다. 정상 준비 완료도 데이터 전체의
power-loss 복구, 서비스 writer quiescence, native UI/Windows나 update/rollback
검증을 대체하지 않는다.

## 실제 통합 검사

workspace Rust 검사와 CLI build 뒤 아래 검사를 실행한다. 두 이미지가 이미
로컬에 존재해야 하며 유지보수 이미지는 검증된 immutable image ID를 사용한다.
이미지를 내려받거나 기존 설치를 대상으로 실행하는 명령이 아니다.

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --locked --bin exhibitos-manager
python3 scripts/test-installation-backup.py \
  --manager "$PWD/target/debug/exhibitos-manager" \
  --maintenance-image 'sha256:<verified maintenance image ID>' \
  --postgres-image 'postgres:18.6@sha256:<verified registry digest>'
```

검사는 합성 설치 metadata를 만든 뒤 실제 native CLI로 설정을 준비하고,
새 PostgreSQL 두 개와 Platform 유지보수 이미지로 암호화·새 경로 복원을
실행한다. 원래 설치 경로와 source DB가 unavailable인 상태에서 5개 파일의
바이트·0600 권한과 실제 복원 DB witness를 확인한다. 실제 Manager engine
설치나 복원 활성화, 원래 artwork, GUI·Windows·Podman·업데이트 검사는 아니다.

결과와 새 private 후보는 임시 경로에 보존하며 Git에 넣지 않는다. 검사에서
생성한 label이 정확히 일치하는 container만 정리한다. 이전 설치·백업은 건드리지
않는다. 실패하면 private 후보와 로그를 확인하고 원인을 해결한 뒤 새 검사로
재검증한다.
