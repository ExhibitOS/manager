# ExhibitOS Manager

로컬 전시 서버의 설치·시작·정지·재시작·상태와 작업 기록을 관리하는
Tauri2/Rust 데스크톱 앱의 개발 구현입니다. 현재 Windows 실환경 검사,
macOS 서명·공증과 배포용 설치 프로그램은 완료되지 않았습니다. 앱의 실제 복원 GUI 조작·취소/재개,
업데이트·rollback과 cloud wizard는 후속 작업입니다.
[설치된 전시의 백업 생성](docs/backup-creation.md)은 개발 CLI에서 Docker 기반
설치의 DB·작품·서명 설정·설치 파일·images를 암호화·인증하며 전시 writer를 정지 상태로 둡니다.
로컬 앱 창의 생성 화면에 같은 adapter를 연결했으며 실제 네이티브 GUI 조작과
Windows/Podman 검증은 남아 있습니다. [새 설치 복원 CLI](docs/backup-restoration.md)는
암호화 사본의 DB·작품·서명 설정·이미지를 원본과 분리된 새 Docker 설치에 복구하고
전시 readiness를 확인하는 개발 경로입니다. 로컬 앱 복원 화면도 같은 코어에 연결했으며 실제 GUI 조작·취소/재개·cold engine과
전체 frozen corpus qualification은 별도 조건입니다. 개발 CLI와 백업 검증 화면의
[암호화 백업 인증 검증](docs/backup-verification.md)은 네트워크 없이 실제 유지보수
이미지를 실행하며 새 비공개 작업 공간에만 복호화합니다. 로컬 앱 창에 검증 화면을 연결했으며 실제 네이티브 GUI 조작 검증은 아직 남아 있습니다. 엔진 실행과 분리된
[업데이트 안전 판정 코어](docs/update-safety.md)는 검증 결과를 입력받아 실패와
중단 복구를 판정하며, 실제 업데이트·서명·복원 기능을 실행하지 않습니다.

[설치 설정 보존](docs/installation-backup.md)은 기존 설치·환경 파일을 암호화 서비스 백업에 포함하기 위한 private 준비 기능이며 전체 백업 완료가 아닙니다.

[데스크톱 사용법](docs/desktop.md)은 runtime bundle 준비, 실행 도구 검사,
실패 복구, 저장 공간 표시와 실제 네이티브 권한 경계를 설명합니다.
웹 preview는 실제 실행 도구를 제어하지 않습니다. 앱은 검증된 Platform OCI
bundle을 별도 프로세스로 실행하며 공개 lifecycle readiness protocol1을
검사합니다. 빌드와 실행에 operations나 Capture 코드는 필요하지 않습니다.

## 개발

Node24.21.0/npm11.19.0, Rust1.99.0 (`rust-toolchain.toml`)을 사용합니다.
macOS 네이티브 빌드는 Xcode compiler/SDK가 필요합니다.

```sh
npm ci
npm run check
npm run test:browser
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
npm run desktop:build
```

macOS 개발 앱 경로는 `target/release/bundle/macos/ExhibitOS Manager.app`입니다.
`npm run desktop:dev`로 개발 앱을 시작합니다. Docker 또는 Podman과 Compose가
별도로 필요하며 설치 여부뿐 아니라 daemon 연결을 검사합니다. 버전과 해시가
검증된 runtime bundle이 준비되기 전에는 서버 설치를 성공으로 표시하지 않습니다.
앱 자체 빌드 성공과 실제 서버 lifecycle·Windows·서명 검증은 구분합니다.
자동 hosted CI나 유료 서비스는 활성화하지 않습니다.

## 데이터와 기여

작업 전 [AGENTS.md](AGENTS.md)를 읽고 `codex/<task>` 브랜치와 검증된 PR을
사용합니다. root별 private 환경 파일과 durable 작업 기록은 Git 밖의
OS app-data/지정 개발 경로에 보존합니다. 정지는 volume을 삭제하지 않습니다.
원본 작품, 토큰, 서명 키, 비밀번호와 raw container 로그를 Git/Issue에 넣지 마세요.
취약점은 활성화된 GitHub private reporting 또는 조직 관리자에게 비공개로
보고합니다. 현재 설치 코드가 실제 사용자 데이터의 복구 검증을 대신하지 않습니다.

## 라이선스

Manager 소유 코드·설정·문서는 [Apache-2.0](LICENSE)입니다. 실행하는 Platform은
AGPL-3.0-or-later이며 별도 이미지의 원문 LICENSE/third-party 고지를 보존합니다.
외부 라이브러리와 작품은 각각 원래 권리를 유지합니다. 이 라이선스는 상표나
사용자 작품의 display/export 권한을 부여하지 않습니다. 현재 저장소는 비공개이며
공개 여부는 source/권리/secret/독립 build 검토 후 결정합니다.
