# Manager 데스크톱 사용법

이 문서는 T08-01의 로컬 설치·시작·정지·재시작·상태와 작업 기록을 다룬다. 백업·복원, 업데이트·rollback, cloud 설정은 이 화면에서 아직 제공하지 않는다. Manager 자체와 실행하는 platform은 서로 다른 라이선스를 유지한다.

## 처음 실행하기

1. Docker 또는 Podman과 해당 Compose 도구를 설치하고 실행한다. 실행 도구 설치 과정의 OS 관리자 승인과 사용자 계정 설정은 컴퓨터 소유자가 수행한다. 설치 여부만으로 사용 가능하다고 표시하지 않으며 실제 엔진 연결을 검사한다.
2. 운영자가 검증한 ExhibitOS runtime bundle을 아래의 지정 경로에 준비한다. 현재 개발 앱은 runtime image를 자동 다운로드하는 설치 프로그램이 아니다. bundle이 없는 상태에서는 설치가 실패하고 복구 안내를 표시한다.
3. `ExhibitOS Manager.app`을 연다. **실행 도구 다시 확인**을 눌러 사용 가능 상태를 확인한다. **저장 공간**에서 디스크 여유 공간과 필요한 여유 공간을 확인한다. 전시 사용량을 측정할 수 없으면 그 사실을 표시한다. 안내 용량은 강제 quota가 아니다.
4. **전시 설치**를 누른다. 설치는 manifest, Compose와 image 해시를 검증한다. **작업 기록**에서 완료 또는 실패 안내를 확인한다. 설치 완료는 서버 시작 완료를 뜻하지 않는다.
5. **시작**을 누른다. 서버 준비가 완료된 뒤 **전시 열기**를 누르면 기본 브라우저에서 지정된 `127.0.0.1` 전시만 연다. 외부 주소나 임의 명령은 입력할 수 없다.
6. **정지**는 container 실행을 중단하며 저장 volume을 삭제하지 않는다. **재시작**은 정지 후 다시 시작한다. 실패하면 안내를 해결한 다음 **실패한 작업 다시 시도**를 사용한다. 앱을 다시 열어도 작업 기록이 남는다. 중단된 작업은 성공으로 표시하지 않는다.

서명·공증이 없는 개발 앱은 macOS가 실행 확인을 요구할 수 있다. 배포 서명·공증과 Windows 설치 검증은 별도 검증 항목이며 개발 빌드 성공으로 대신하지 않는다. 시스템 보안 기능을 일괄 해제하지 않는다.

## 운영자가 준비하는 경로

기본 root는 Tauri가 반환하는 OS app-data 경로 아래의 `local-runtime`이다. macOS identifier는 `org.exhibitos.manager`이다. 개발·검증은 실행 프로세스의 `EXHIBITOS_MANAGER_ROOT`에 절대 경로를 지정해 사용자 데이터와 격리한다. 프런트엔드는 경로를 바꾸거나 임의 Compose 파일을 고를 권한이 없다.

root 안의 `bundle/manifest.json`, `bundle/compose.yaml`과 manifest에 지정된 image archive는 신뢰한 배포 채널에서 받아 준비한다. 자체 파일 해시는 잘못된 파일을 감지하지만 신뢰하지 않은 배포자를 신뢰하게 만들지는 않는다. 운영자는 bundle 출처와 게시된 해시를 별도로 검증한다. manifest의 version/protocol, image digest, archive 크기·해시, services, ports, readiness URL과 최소 여유 공간은 lifecycle core의 검증을 통과해야 한다. Compose 프로젝트와 저장 volume은 해당 root의 관리 대상으로 격리한다.

Finder에서 실행할 때도 backend는 표준 Docker/Podman 설치 경로를 검사한다. 엔진 daemon이 정지했거나 Compose가 없으면 안내에 따라 실행 도구를 준비하고 다시 확인한다. Manager는 관리자 암호나 registry token을 입력받지 않고 raw container 로그를 화면에 노출하지 않는다.

실행 도구가 응답하지 않으면 `ENGINE_UNAVAILABLE`, 접근이 거부되면
`ENGINE_PERMISSION`, 제한 시간 안에 응답하지 않으면 `ENGINE_TIMEOUT`으로
안내한다. Compose provider가 실행되지 않으면 `COMPOSE_UNAVAILABLE`이다.
버전 문자열을 정상적으로 확인한 뒤 지원 범위를 벗어났을 때만
`VERSION_MISMATCH`를 표시하며, raw 오류 출력이나 credential을 반환하지 않는다.

manifest에 `preferredEngine`이 명시되면 그 패키지를 만든 실행 도구를 요구한다.
해당 도구가 꺼져 있어도 다른 도구로 자동 전환하지 않는다. 이 필드가 없는 기존
bundle은 사용 가능한 도구를 선택할 수 있다. 설치의 image 검증이 모두 통과한
뒤에만 engine 선택을 기록하며, 실패한 초기 설치의 임시 선택은 retry를 고정하지
않는다. 설치 완료된 root의 실행 도구는 유지하며, 다른 producer를 요구하는
bundle을 덮어 설치하는 동작은 거부한다.


## 개발 빌드와 검사

Node 24.21.0/npm 11.19.0과 `rust-toolchain.toml`의 Rust를 사용한다. macOS의 네이티브 compiler/SDK가 필요하다. Windows는 해당 OS의 Tauri 빌드 요구 사항과 실제 설치·실행 검증이 추가로 필요하다.

```sh
npm ci
npm run check
npm run test:browser
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
npm run desktop:build
```

macOS 개발 결과는 `target/release/bundle/macos/ExhibitOS Manager.app`이다. `npm run desktop:dev`는 로컬 Vite 서버와 네이티브 앱을 함께 시작한다. `npm run dev`만 실행해서 여는 브라우저 화면은 디자인·접근성 확인용이며 실제 lifecycle 권한이 없어 제어 버튼이 비활성화된다. browser test는 실제 네이티브 작업 성공을 증명하지 않는다.

프로젝트 안의 격리된 Rust 도구를 사용하는 경우 다음 환경을 설정한 같은 terminal에서 검사한다. 경로는 자신의 checkout에 맞춘다. 토큰·암호를 명령 인수에 넣지 않는다.

```sh
export RUSTUP_HOME="/absolute/ExhibitOS/.local/toolchains/rustup"
export CARGO_HOME="/absolute/ExhibitOS/.local/toolchains/cargo"
export PATH="$CARGO_HOME/bin:$PATH"
export EXHIBITOS_MANAGER_ROOT="/absolute/isolated-manager-runtime"
npm run desktop:build
```

## 데스크톱 권한 경계

앱은 번들된 로컬 webview 한 개만 사용한다. 원격 문서 탐색을 차단하고 CSP와 main-window 전용 capability를 적용한다. Rust IPC는 상태, 엔진 검사, 설치, 제한된 lifecycle action, 작업 기록, 안전한 진단 기록, 지정 전시 열기의 일곱 명령만 허용한다. 파일 시스템·shell plugin과 임의 URL 열기는 노출하지 않는다. 실행 작업은 blocking worker에서 처리해 화면의 상태 조회를 유지한다.

권한 설정의 근거는 Tauri의 [보안 경계](https://v2.tauri.app/security/), [capabilities](https://v2.tauri.app/security/capabilities/)와 [permissions](https://v2.tauri.app/security/permissions/) 문서다. frontend 입력 검사는 사용성을 위한 추가 검사이며 엔진 실행 권한은 Rust lifecycle core에서 검증한다.

## 검증 범위 기록

실제 명령 결과와 기기·엔진 정보는 해당 commit의 검증 기록에 남긴다. 웹 preview, Rust 단위 검사, macOS 앱 빌드, 네이티브 GUI, 실제 engine 설치·시작·정지, Windows, 서명·공증을 구분한다. 하나의 성공을 다른 검증의 성공으로 기록하지 않는다. 현재 단계의 GUI에는 백업·복원이나 업데이트 완료 메시지를 만들지 않는다.

## Podman 개발 검사

실제 검사 후보는 rootless Podman6.1.3와 독립 Compose5.5.1 제공자이다.
같은 실행 환경에서 `PODMAN_COMPOSE_PROVIDER`를 해당 독립 CLI 경로로 설정한다.
필요한 경우 `CONTAINER_CONNECTION`으로 검증용 named connection을 지정한다.
Docker Desktop 서버와 privileged socket helper를 설치할 필요는 없다.
Compose1.x 제공자는 이 adapter의 JSON 계약에 맞지 않아 준비 완료로 표시하지 않는다.
이미지 archive의 engine별 ID는 서로 다를 수 있으므로 해당 producer의 bundle을
사용한다. sha256 접두어 차이만 정규화하며 다른 digest는 거부한다.
기존 VM·연결·사용자 데이터는 변경하거나 자동 삭제하지 않는다.

## 상태와 실패 안내 읽기

화면 위 **다음 할 일**은 확인된 설치·실행 상태에 따라 다음 조작을 안내합니다. 실행 중에는 중복 작업을 막고 **작업 진행과 복구** 영역에 저장된 진행률을 표시합니다. 진행률은 backend 작업 기록이며 남은 시간을 예측하지 않습니다. 실패·중단 후에는 복구 안내를 해결한 다음 **실패한 작업 다시 시도**를 누릅니다. 상태 확인을 다시 하는 것은 설치나 시작 작업을 재실행하는 동작이 아닙니다.

서비스의 **실행 중**과 **응답 정상**은 서로 다른 상태입니다. 서비스가 실행 중이어도 서버 준비가 완료되기 전에는 **전시 열기**가 활성화되지 않습니다. 알 수 없는 서비스 상태는 확인 필요로 표시하며 성공으로 추측하지 않습니다. **안전한 진단 기록**을 펼쳐 부가 정보를 확인할 수 있습니다.
