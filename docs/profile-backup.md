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

실패 진단: `PROFILE_DESTINATION_EXISTS`는 실제 기존 파일과의 충돌일 때만 반환합니다. 대상 경로 조회 또는 새 파일 생성이 실패하면 `PROFILE_DESTINATION_UNAVAILABLE`로 경로·권한·여유 공간을 확인하세요. 이미 생성한 암호화 pending의 publish/link 또는 동기화 실패는 `PROFILE_WRITE_UNCERTAIN`이며 성공으로 처리하지 않습니다. pending과 기존 사본을 보존하고 실제 파일 상태를 확인한 뒤 새로운 이름으로 재시도하세요. raw OS 오류나 private 경로는 반환하지 않습니다.

## 스트리밍 envelope 개발 옵션

`backup-stream` 명령은 동일한 설정 snapshot을 새 `ExhibitOS-stream-v1` envelope에 저장합니다. 기본 `backup`은 기존 profile-v1 형식을 유지하며 현재 `restore`는 두 형식을 모두 읽습니다. 이전 binary는 새 envelope를 읽을 수 없으므로 새 사본에는 현재 CLI를 사용하세요. 선택 목록 payload·64MiB 사본 한도·앱 종료/공간 잠금·가역적 복원·외부 private 키 조건은 그대로입니다.

```sh
./target/release/exhibitos-profile --profile '<canonical private app-data directory>' backup-stream \
  '<external 32-byte key file>' '<new external profile archive>' --apps-closed
```

암호화 codec은 최대1MiB 단위로 읽고 각 record의 AES-GCM nonce를 OS 난수로 생성합니다. archive identity, payload 용도, record 순서·종류·길이를 associated data로 인증합니다. 별도 종료 record는 전체 plaintext 길이와 SHA256을 인증하며 종료 record 없는 정상 prefix나 추가 trailing bytes를 완료로 인정하지 않습니다. 사본 공개 전에 전체 인증·닫힌 schema 검사를 수행합니다.

이 옵션은 **관리 공간 설정만** 보관합니다. 기존 snapshot은 여전히 제한된 메모리에 직렬화되며 아직 journal·불완전 candidate·DB/blob/engine volume을 수집하지 않습니다. 이 codec을 재사용할 별도의 관리 작업 사본은 root/profile/session 잠금, helper/writer 정지 확인, host 파일/외부 volume 구분, streaming manifest, private quarantine과 가역적 canonical root 복원까지 연결하고 실제 검사해야 합니다. codec 또는 설정 round trip을 전체 후보 복구의 증거로 사용하지 마세요.

## Profile 교체 중 경로 잠금

현재 앱과 offline CLI는 기존 profile 안의 `profile-session.lock`과 함께 부모 폴더의 `.exhibitos-profile-session-<canonical profile path SHA256>.lock`을 보유합니다. 앱은 같은 경로의 공유 잠금을 **profile 생성 전**에 확보하며 offline 작업은 배타 잠금을 확보합니다. profile을 이름 변경하거나 새 inode로 교체해도 같은 canonical 경로의 잠금은 계속 유지됩니다. 부모 경로 alias는 canonicalize하여 같은 anchor를 사용합니다. 현재 설정 복원은 여전히 pointer 복원이며 이 변경이 후보 데이터/폴더 교체를 실행하지는 않습니다.

Anchor는 파일 이름이 아닌 내부 경로 기반 fence입니다. 0600·현재 UID·일반 파일·단일 hardlink·inode 일치를 검사하고 symlink/권한 오류를 거부합니다. 부모는 현재 UID 소유이며 다른 사용자가 쓰지 못하는 폴더 또는 현재 UID/root가 소유한 sticky 공유 임시 폴더여야 합니다. 기존 내부 잠금도 유지하므로 이전 앱의 열린 세션과 충돌하면 거부합니다. 단, 이전 binary는 외부 anchor를 이해하지 못하므로 향후 폴더 교체 작업 중 이전 앱을 새로 실행하면 안 됩니다. 모든 이전 앱/CLI/외부 writer 종료 동의는 그대로 필요합니다. Windows·관리자/비협조적 외부 namespace 변경을 검증한 보안 격리로 주장하지 않습니다.

Anchor는 복원할 데이터가 아닌 잠금 인프라이므로 profile 사본에 포함하지 않으며 unlink·이동·덮어쓰기로 정리하지 않습니다. 검사 실패나 `PROFILE_BUSY`에서는 기존 폴더·잠금·기록을 유지하세요. 부모 폴더 자체를 이동한 경우는 다른 canonical 경로이며 자동 migration하지 않습니다. 전체 작업 기록·후보 사본과 가역적 root 교체는 아직 후속 구현입니다.


## Compact full-host envelope (version2)

New full-host checkpoints use `ExhibitOS-stream-v2`: each bounded1MiB logical record is independently zlib-compressed when smaller, then authenticated/encrypted. Incompressible records remain raw. AES-GCM binds version, archive identity, context, record order, type and encoded size; the final authenticated count and digest cover the complete expanded stream. Profile-only backup formats remain unchanged. Current readers accept retained version1 full-host archives; older binaries cannot read version2, so keep the matching verified CLI with the archive.

Compression never excludes files or weakens byte/hash/mode validation, quiescence, source identity, publication or recovery gates. Decoding caps each expanded record at1MiB, checks exact decompressor input/output and stream termination, and enforces total logical quotas. Host extraction authenticates the entire stream before checking available space against expanded bytes; it then authenticates again into private unpublished staging. Ciphertext size cannot authorize a smaller extraction budget. Whole-runtime export headroom and the operations free-space floor are unchanged. Compression ratios depend on actual content; no retained legacy recovery baseline should be retired until a replacement is fully restored and verified.
