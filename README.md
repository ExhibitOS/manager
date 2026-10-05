# ExhibitOS Manager

로컬 전시 서버의 설치·시작·정지·재시작·상태와 작업 기록을 관리하는
Tauri2/Rust 데스크톱 앱의 개발 구현입니다. Windows Docker 기본 설치·시작·정지·재시작·API 연결과 진행 표시를 실제 사용자 환경에서 확인했습니다. 전체 Windows 복구·업데이트·설치 프로그램 검증,
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

[영구 업데이트 기록](docs/update-intent.md)은 서명·실제 아티팩트 검증을 연결하고
준비·적용·health·복원·롤백 상태와 중단 복구를 private 저널에 저장합니다.
실제 업데이트 실행 어댑터와 운영 환경의 전체 복원 검증은 아직 남아 있습니다.

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

관리 공간 전환과 보존·복구 경계는 [관리 공간 선택](docs/installation-selection.md)을 참조하세요.

[실패·중단 helper 확인](docs/helper-reconciliation.md)은 해당 작업의 보조 실행만 정지하고 데이터를 보존하는 복구 경로입니다. 실행 중 취소·자동 재개와 원래 백업·복원 성공 판정은 별도입니다.

진행 중 백업 생성·복원의 [취소 요청과 정지 확인](docs/maintenance-cancellation.md)을 지원합니다. 요청과 정지 완료를 구분하며 후보와 데이터를 보존합니다.

앱을 닫은 상태의 [암호화 관리 공간 설정 백업·복원](docs/profile-backup.md)은 전시 데이터 사본과 별도로 선택 목록·이력·공간 정보를 보존하는 개발 CLI입니다.

중단된 백업·복원의 [명시적 새 작업 재시도](docs/maintenance-retry.md)는 실패 후보와 이전 journal을 보존하는 개발 CLI와 로컬 앱 화면입니다. 복원은 등록된 별도 공간에서 실행하며 앱 화면의 실제 네이티브 GUI·Windows 검증은 남아 있습니다.

새 작업 ID 연결 전 앱이 종료된 경우의 [재시도 진단·복구](docs/retry-recovery.md)는 준비·예약과 후보 hash를 확인해 원래 기록을 보존합니다. 앱의 진단 화면도 같은 코어를 사용하며 실제 네이티브 GUI와 전체 복원·update 인수 검증은 별도입니다.

[Host profile 전체 파일 사본·비활성 추출 개발 CLI](docs/host-checkpoint.md)는 작업 journal과 후보 bytes를 별도 스트리밍 사본에 보존합니다. 외부 engine 볼륨과 live profile 활성화는 포함하지 않습니다.

[서명된 Runtime 릴리스 검증 CLI](docs/signed-releases.md)는 고정된 공개키 정책과 실제 artifact hash를 검사합니다. [별도 신뢰 기록 CLI](docs/release-trust.md)는 프로필 복원 영역 밖에 정책·폐기 키·수락 버전 기록을 저장합니다. 다운로드·업데이트 실행·backup/rollback 검증은 후속 연결 조건입니다.

[업데이트 준비 기록 CLI](docs/update-intent.md)는 검증한 릴리스와 대상·백업·원본 계획을 수락 기록과 함께 저장합니다. 실제 적용·마이그레이션·건강 확인·롤백은 후속 실행 단계입니다.

[정지 원본 DB 사본 개발 CLI](docs/source-database-snapshot.md)는 원본 네이티브 볼륨을 읽기 전용으로 새 고유 볼륨에 복사해 내용·권한·소유자와 정상 종료 상태를 검사합니다. 현재 DB/blob 논리 인벤토리 비교와 전체 업데이트 preflight는 후속 조건입니다.

[현재 원본 DB·작품 인벤토리 개발 CLI](docs/source-inventory.md)는 원본을 정지 상태로 유지하고 새 DB 사본과 읽기 전용 작품 볼륨에서 인증된 백업의 전체 데이터 인벤토리를 대조합니다. 전체 설정/preflight·업데이트 실행은 별도입니다.

[현재 설정 전체·이미지 바이트 개발 CLI](docs/configuration-inventory.md)는 지원되는 설정7개의 현재 bytes와 새 Engine image export를 인증된 백업에 대조합니다. 누락·추가 설정과 바이트가 다른 export는 거부하며 전체 preflight·업데이트 실행을 대신하지 않습니다.

[실제 개발 Runtime OCI 패키지 검증](docs/genuine-runtime-release.md)은 로컬 이미지 아카이브의 구조·바이트·마이그레이션과 개발 서명을 검사합니다. 실제 업데이트 실행·health·rollback은 후속 검증입니다.

[서명된 Runtime 비공개 staging](docs/runtime-staging.md)은 새 고유 사본과 읽기 전용 핸들에서 실제 바이트를 검증하는 Unix 개발 CLI입니다. OCI import·실제 적용 권한은 제공하지 않습니다.

[개발 OCI 캐시 가져오기](docs/development-oci-import.md)는 같은 staged 파일 핸들에서 구조 검증과 Docker 가져오기를 수행하고 기존 서비스·볼륨·태그를 보존합니다. 설치 업데이트·실제 health·rollback은 후속 단계입니다.

[실제 Runtime 릴리스 계획과 복원 후보 검사](docs/genuine-update-plan.md)는 실제 artifact에 새 준비 계획을 연결합니다. 복원 후보는 원본 버전 Runtime을 검사하며 신규 target 적용·health·rollback은 별도입니다.

[독립 사본 target Runtime 검사](docs/target-runtime-probe.md)는 신규 버전과 이전 버전의 기본 동작을 별도 synthetic 사본에서 검사합니다. 실제 설치 전환·전체 호환성·rollback 인수 조건은 별도입니다.

[호스트·신뢰 묶음 사본](docs/paired-host-trust-checkpoint.md)은 같은 cooperative 잠금에서 두 암호화 사본을 만들고 비활성 경로 추출을 지원합니다. 현재 외부 서비스 볼륨 백업·authority 복원·실제 업데이트 인수 조건은 별도로 유지합니다.

[같은 fence의 현재 원본 결합 관측](docs/source-recovery-bundle.md)은 DB/blob 앞뒤와 전체 설정·이미지 바이트를 함께 인증 백업에 대조합니다. Host/trust/data 전체 복원 지점과 Applying 권한은 후속 통합 조건입니다.

[원본·호스트·신뢰 기록 통합 관측](docs/source-host-trust-checkpoint.md)은 같은 cooperative fence 안에서 현재 서비스와 인증 백업을 대조하고 호스트·신뢰 사본을 생성한 뒤 다시 대조합니다. 새 외부 volume 백업·전체 복원·실제 업데이트 완료는 별도 조건입니다.

[만료 개발 계획 갱신](docs/development-plan-renewal.md)은 비활성 새 후보를 등록하고 기존 floor·키 폐기·ID 예약을 보존하는 명시적 개발 절차입니다. 새 후보의 실제 서비스 복원과 업데이트 검증은 이후 수행합니다.

[Windows private-profile foundation](docs/windows-private-profile.md) is an unqualified native security component; managed Windows profiles and full lifecycle acceptance remain incomplete.

설치·시작·정지·재시작을 요청하면 설치와 실행 패널에서 즉시 진행 상태와 해당 버튼의 `… 중` 표시를 제공합니다. 코어 응답을 기다리는 동안에는 완료율을 추측하지 않는 진행 막대를 표시하며, 완료·실패 결과를 같은 위치에 남깁니다. 응답 시간과 실제 네이티브 동작은 실행 환경에 따라 다르므로 브라우저 합성 지연 검사는 Engine 작업 성공의 증거가 아닙니다.

남은 Windows 검사는 [한 번의 검증 배치](docs/windows-verification-batch.md)로 모아 준비하며, 이미 받은 성공 결과는 같은 소스에서 반복 요청하지 않습니다. `npm run test:receipt`는 읽기 전용 provenance 수집의 변조·경로탈출 회귀를 확인합니다.

Stopped candidate data can also be checked with bounded tmpfs instead of accumulating persistent database snapshots: see [candidate inventory without persistent snapshots](docs/genuine-update-plan.md#candidate-inventory-without-persistent-snapshots). This opt-in diagnostic preserves full physical and logical checks; it does not authorize an update or complete platform qualification.
