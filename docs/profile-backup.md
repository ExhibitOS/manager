# 앱 종료 상태의 관리 공간 설정 백업·복원

모든 Manager 앱을 종료한 뒤 `exhibitos-profile` 개발 CLI로 관리 공간 선택 목록, 이전 선택 이력, 등록된 공간의 상대 위치와 가용 상태를 함께 백업합니다. 새 버전의 모든 관리 앱 창은 실행 내내 shared profile-session.lock을 보유하며, 백업·복원은 배타 잠금과 profile/등록 공간의 operation lock을 확보해야 합니다. 앱이 열려 있거나 작업이 실행 중이면 거부합니다. 이전 버전 앱과 직접 파일 편집 도구도 반드시 종료해야 합니다. 관리자나 비협조적인 외부 파일 쓰기를 격리하는 filesystem snapshot은 아닙니다.

외부 32바이트 무작위 키로 AES-256-GCM 인증 암호화합니다. 각 사본에 OS 난수 96비트 nonce를 생성하며 형식 magic은 associated data로 인증합니다. 별도의 설정 백업용 키를 준비하고 보관하세요. 키를 잃으면 사본을 복구할 수 없습니다. 원문 구현/API는 [RustCrypto aes-gcm](https://docs.rs/aes-gcm/0.10.3/aes_gcm/)을 따릅니다. 고정 의존성의 라이선스 원문과 checksum은 licenses/profile-crypto에 보존합니다.

## 준비와 실행

1. 앱과 해당 관리 공간의 CLI 작업을 모두 종료합니다. macOS에서는 Manager 메뉴의 종료 또는 Command+Q를 사용하세요. 창만 닫고 앱 프로세스가 남아 있으면 session 잠금도 유지됩니다. 진행 중인 백업·복원은 먼저 취소/정지 상태를 확인하고 후보를 보존하세요. 전시 데이터 백업은 별도 [서비스 백업](backup-creation.md)으로 일관되게 보관합니다. 설정 사본을 만들기 위해 전시 서버를 자동 정지하지는 않습니다.
2. profile은 앱의 OS app-data 디렉터리입니다. `local-runtime`, `installations`, `installation-selection.json`을 가진 실제 절대 경로를 확인합니다. 경로를 추측하거나 unrelated 폴더를 대상으로 하지 마세요.
3. 사본과 키는 profile 밖의 별도 0700 디렉터리에 둡니다. 키는 내용이 정확히 32바이트인 0600 일반 파일이어야 합니다. symlink·hardlink·완화된 권한을 거부합니다. 키 내용은 채팅·Git·로그에 넣지 않습니다. 키와 사본의 보관 위치를 분리하고 키는 OS secret store 또는 안전한 오프라인 위치에 별도로 보관하세요.
4. 백업 파일은 아직 존재하지 않는 새 이름을 사용합니다. 기존 사본을 덮어쓰지 않습니다.

```sh
cargo build --release --locked -p exhibitos-lifecycle --bin exhibitos-profile
./target/release/exhibitos-profile --profile '<canonical private app-data directory>' backup \
  '<external 32-byte key file>' '<new external profile archive>' --apps-closed
./target/release/exhibitos-profile --profile '<canonical private app-data directory>' restore \
  '<external 32-byte key file>' '<existing profile archive>' --apps-closed
```

`--apps-closed`는 모든 앱·외부 설정 writer가 닫혔고 기존 공간과 사본을 보존한다는 동의입니다. 잘못된 키·변조·잘림·알 수 없는 형식·이력 해시 오류는 선택 목록 교체 전에 거부합니다. 128개 등록 공간, 1024개 이력과 64MiB 인증 envelope 한도를 사용하며 초과 시 자동 삭제하지 않습니다.

## 복구 범위와 가역성

암호화 사본은 profile 폴더가 없어도 인증할 수 있습니다. 대상 profile은 운영자가 준비한 기존 0700 폴더여야 합니다. 모든 공간 경로는 검증된 UUID 기반 상대 경로이며 임의 외부 위치로 매핑하지 않습니다. 다른 디스크의 profile로 설정을 복원할 수 있지만 이 도구가 DB·작품·키·환경 파일·공간 폴더를 복구하거나 이동하지는 않습니다. 별도 전시 백업에서 데이터 공간을 복구하고 실제 DB/blob 일치·로그인·writer 상태를 확인해야 합니다. 폴더가 없는 등록 공간은 목록에 남고 앱은 unavailable로 표시하며 생성하거나 시작하지 않습니다.

복원은 선택한 시점의 목록을 정확히 복구합니다. 그 이후 추가된 공간의 폴더·데이터와 기존 이력은 그대로 유지합니다. 이전의 현재 목록은 손상된 JSON이어도 `profile-restore-UUID/previous-registry.bin`에 0600으로 보존합니다. 이전 목록이 유효하면 SHA256으로 확인되는 selection-history에도 보존하여 다음 암호화 사본에 포함합니다. 손상된 원래 bytes는 진단 기록으로 남고 typed history에는 넣지 않습니다. 인증된 새 목록도 같은 보존 디렉터리에 저장한 뒤 이력을 확인/추가하고 선택 포인터를 원자적으로 교체합니다. receipt는 마지막 교체·동기화 뒤에만 기록합니다. 중간 오류·전원 장애가 있으면 완료를 추측하지 말고 앱을 닫아 둔 채 현재 포인터와 보존 기록·임시 후보를 검사하세요. 실제 powerloss 검증을 완료했다는 뜻은 아닙니다.

복원 전 현재 선택 목록을 다시 사용하려면 앱을 닫고 해당 보존 파일의 권한·내용·등록 공간을 확인합니다. 현재 포인터를 또 별도 보존하고 private 임시 파일에 이전 bytes를 복사한 뒤 동일 폴더에서 원자적으로 교체·디렉터리 동기화합니다. 새로 추가된 공간의 실제 데이터는 언제나 그대로 둡니다. 원본과 실패 후보·사본·이력을 자동 만료하거나 삭제하지 않습니다.

이 경로는 Unix/macOS 개발 CLI와 새 native controller의 잠금 연동입니다. Windows ACL/잠금, 실제 GUI, 배포용 도구 서명·설정 메뉴 연결은 별도 조건입니다. Git bundle에는 사본·키·프로필·DB/blob·volume·private journals가 포함되지 않습니다. 전체 T08-02 backup/OEX/update/rollback 완료나 운영 복원 지점으로 표시하지 않습니다.

## 개발 검사

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --release --locked -p exhibitos-lifecycle --bin exhibitos-profile
python3 scripts/test-profile-backup.py --profile-cli '<built exhibitos-profile>'
```

실제 CLI 검사는 새로운 private 합성 profile과 외부 키·사본만 사용합니다. 앱 session/작업 lock의 cross-process 상호 배제, 암호문과 no-overwrite, wrong key/tamper/truncated, 원본 profile 경로 부재, 이전 포인터/추가 공간 witness 보존, 0바이트 목록 복구, history 충돌, missing 공간의 생성 금지와 키 권한을 검사합니다. 모든 시험 원본·사본·키·실패 후보를 보존합니다. Rust 검사는 실제 여러 controller의 수명 동안 shared lease 유지, 앱 재열기의 unavailable 표시, 인증된 중복/unknown payload 거부를 추가로 확인합니다. 실제 native GUI와 Windows·전원 차단은 독립 gate입니다. 실행하지 않은 결과를 통과로 기록하지 않습니다.

설정 사본은 로컬 private 파일의 atomic write/link와 flock 동작에 의존합니다. 네트워크 filesystem/동기화 도구의 외부 변경까지 보장하지 않습니다. 부분 publish 뒤 pending hardlink가 남았으면 사본과 pending의 실제 byte hash를 별도 확인하고 앱을 닫은 상태로 inspect하세요. 확인되지 않은 pending을 자동 삭제하거나 정상 사본으로 추측하지 않습니다. 사용자는 암호화 사본과 외부 키의 별도 보관·실제 복원 검사를 유지해야 합니다.

기존 관리 profile 디렉터리의 POSIX 소유권·0700 권한이 예상과 다르면 새 앱은 자동으로 권한을 고치지 않고 거부합니다. 이전 버전으로 초기화된 정상 private profile의 목록 형식과 공간 ID는 유지합니다. 완화된 권한은 원본과 OS 접근 정책을 확인한 뒤에만 수정해야 합니다. 보존한 손상 목록·진단 디렉터리는 typed 암호화 설정 inventory 밖이며 필요한 경우 별도 private 보관 정책을 유지합니다. 기본 사본 확장자 .exb와 key.bin/*-key.bin, profile metadata 이름은 Git-ignore하지만 임의 이름의 secret을 식별하는 보안 경계는 아닙니다. 항상 소스 저장소 밖에 보관하세요.
