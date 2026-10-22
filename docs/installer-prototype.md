# Windows/macOS 설치 프로그램 개발 파이프라인

이 경로는 기존 T08-03의 설치 파일과 source version 연결을 구현한다. 배포용 서명·공증과 실제 설치 검증이 끝난 제품으로 표시하지 않는다. Windows x64는 NSIS `.exe`, macOS arm64/x64는 `.dmg`를 해당 운영체제에서 빌드한다. 기존 `desktop:build` 개발 앱 명령은 유지한다.

## 준비와 실행

Node24.21.0/npm11.19.0, 저장소의 Rust toolchain과 네이티브 개발 도구가 필요하다. macOS에서는 전체 Xcode SDK를, Windows에서는 MSVC Build Tools와 WebView2 개발 환경을 사용한다. Docker/Podman은 설치 프로그램 제작과 별개로 전시 서버 실행에 필요하다.

검토한 커밋을 별도의 깨끗한 체크아웃에 준비하고 다음 명령을 실행한다. operations나 Capture 저장소는 필요하지 않다.

```sh
npm ci
npm run test:installer
npm run installer:plan
```

영수증을 체크아웃 밖의 새 경로에 저장한다. macOS 예시는 다음과 같다.

```sh
node scripts/build-installer.mjs --build > /private/tmp/exhibitos-manager-installer-$(date +%Y%m%d-%H%M%S).json
```

Windows PowerShell에서는 다음과 같이 저장한다. 기존 증거 파일을 덮어쓰지 않도록 새 시각을 사용한다.

```powershell
$receipt = Join-Path $env:TEMP ("exhibitos-manager-installer-" + (Get-Date -Format "yyyyMMdd-HHmmss-fff") + ".json")
node .\scripts\build-installer.mjs --build > $receipt
if ($LASTEXITCODE -ne 0) { throw "Installer build failed; preserve the receipt and output for diagnosis." }
```

`npm`이 추가하는 명령 안내가 없는 JSON이 필요하면 두 운영체제 모두 `node scripts/build-installer.mjs --build`를 직접 호출한다. 빌드 로그는 stderr, 성공 영수증은 stdout에 출력한다. 영수증 파일은 새 경로여야 한다. 스크립트는 기존 설치 산출물을 지우거나 덮어쓰지 않는다.

`installer:plan`은 OS/architecture, version, commit/tree, 입력 파일 hash와 실제 명령을 검사하며 빌드를 실행하지 않는다. `installer:prototype`은 같은 Tauri CLI를 명시적으로 `--ci --bundles nsis` 또는 `--ci --bundles dmg`와 Cargo `--locked`로 실행한다. 서명 관련 환경 변수는 자식 프로세스에 전달하지 않으며 서명·updater 설정이 있는 구성은 이 prototype 명령에서 거부한다.

## 결과와 보존

산출물은 Windows `target/release/bundle/nsis`, macOS `target/release/bundle/dmg`에 생성한다. macOS Tauri가 남기는 정확한 보조 파일 `bundle_dmg.sh`, `icon.icns`는 `packagingAuxiliaries`에 별도로 이름·크기·SHA256을 기록하며 설치 파일로 세지 않는다. 그 외 예상하지 않은 파일과 symlink/hardlink는 계속 거부한다. 영수증은 실제 각 설치 파일의 이름·크기·SHA256과 source commit/tree, version 및 입력 파일 hash를 기록한다. 일반 경로·비밀번호·키 내용은 영수증에 넣지 않는다. source와 도구·파일 바이트가 제작 도중 바뀌면 성공 영수증을 내지 않는다. symlink/hardlink 설치 파일과 기존 출력도 거부한다.

깨끗한 source에만 실행한다. Windows의 기존 자동 생성 permission 파일에 `M` 표시가 있으면 보존하고 별도 checkout을 사용한다. 기존 사용자 변경을 reset하거나 현재 앱·복구 자료를 삭제해 실행 조건을 맞추지 않는다. 실패 시 부분 출력과 로그를 보존하고 다음 시도는 별도 검토한 깨끗한 checkout에서 수행한다.

결과의 `distribution`은 `unsigned-prototype`, `productionSigned`는 `false`, `signatureVerification`은 `not_run`이다. 이는 배포 서명 증거가 없다는 뜻이며 macOS 도구가 만드는 로컬 ad-hoc code signature를 배포용 신원 검증으로 간주하지 않는다. 실제 NSIS/DMG 제작·설치·제거·업데이트와 offline local start는 각각 해당 운영체제에서 별도로 확인해야 한다. 합성 Node 검사는 이 결과를 대신하지 않는다.

## 서명·공증과 배포

현재 prototype 명령에는 운영 서명 키를 입력하지 않는다. 서명 계정이 준비되면 별도 승인된 배포 절차에서 검토한 동일 source version을 패키징하고 Windows Authenticode 검증 또는 macOS Developer ID 서명·notarization·stapling·Gatekeeper 검사를 수행한다. 계정·인증서·키는 OS/비밀 저장소에 보관하며 Git과 영수증에 넣지 않는다. 서명 완료 파일의 SHA256을 다시 계산하고 실제 검증 결과와 source version을 함께 연결한다.

실제 signed 결과와 clean install/uninstall/update, offline Runtime 시작 증거가 갖춰지기 전에는 production 다운로드나 검증 완료 문구를 게시하지 않는다. 현 단계의 다운로드 안내는 검토된 개발 산출물에만 적용한다. 설치 후 전시 서버 준비 방법은 [데스크톱 사용법](desktop.md), Windows의 남은 실제 검사는 [검증 배치](windows-verification-batch.md)를 따른다. Hosted CI와 유료 서명 서비스를 이 명령으로 활성화하지 않는다.

## 기존 App의 로컬 개발 서명

이미 빌드하고 source·파일 목록을 검증한 macOS App은 별도의 [Apple Development 서명·DMG 경로](development-signing.md)를 사용할 수 있습니다. 원본과 vendor 자료를 보존하고 새 복사본에만 서명합니다. 기존 미서명 prototype 명령의 제한과 production 서명·공증 완료 조건은 유지합니다.

## 개발 앱에서 공식 서명 의도 차단

`desktop:build`는 기존 remote 준비 → Tauri app 빌드를 `scripts/build-desktop.mjs`로 실행합니다. 무서명, `-` ad-hoc, 이름이 명시된 Apple Development 로컬 경로는 유지합니다. Developer ID/분류할 수 없는 identity, 인증서 import·Apple 공증 설정, updater signing 설정 또는 Tauri 외부 구성 override가 있으면 remote 준비와 빌드 전에 실제 update-runtime manifest를 검사합니다. 현재 `private-validation-only` 산출물은 명시적으로 거부하며, 누락·unknown·임의 `public` 변경과 unsafe/과대 manifest도 거부합니다. 환경 변수의 값은 오류에 출력하지 않습니다. certificate SHA1의 개발 identity 확인은 기존 별도 개발 서명 도구를 사용합니다.

`node scripts/build-desktop.mjs --plan`은 같은 경계를 검사하되 준비·빌드는 실행하지 않습니다. 계획 출력은 단계와 전달 인자 수만 표시하며 인자 값은 출력하지 않습니다. 공식 환경에서의 거부는 현재 runtime 재배포 자격이 없다는 뜻입니다. 공식 서명·공증·공개 pipeline 구현이나 자격 완료를 의미하지 않으며 새로운 public flag나 허용 목록을 만들지 않습니다. 고정 archive와 NOTICE가 함께 결속된 `pinned-notices-qualified-v1` producer/native 계약만 공식 의도 사전 검사를 통과할 수 있습니다. 기존 private 산출물과 임의 `public` 변경은 계속 거절합니다. 자격 범위와 원문 근거는 [runtime 배포 자료](release-sbom.md#fixed-runtime-distribution-qualification)를 따릅니다. 직접 Tauri CLI나 외부 수동 서명을 통제하는 OS 보안 경계는 아니며, 공식 게시 전 기존 실제 승인·검증 조건은 유지합니다.

기존 `desktop:build -- --debug -- --locked` 같은 Tauri/Cargo 인자는 순서 그대로 전달합니다. 첫 literal `--` 이전의 Tauri `--config`, `-c`, `--config=…` override는 별도 구성의 서명·runtime 변경을 사전 분류할 수 없으므로 공식 의도로 검사합니다. 오류 JSON은 값 없이 `phase`, `commandsStarted`, `commandsCompleted`, `buildExecuted`로 준비 전 거부와 실제 명령 시작 뒤 실패를 구분합니다.

인증서 import 경계는 설치된 pinned Tauri2.12.1 CHANGELOG의 `APPLE_CERTIFICATE` identity 추론 항목과 같은 CLI binary의 `APPLE_CERTIFICATE_PASSWORD` 상수에 근거합니다. `createUpdaterArtifacts: "v1Compatible"`도 같은 버전의 config schema에 있는 실제 updater 요청으로 처리합니다. 이 검사는 인증서를 열거나 import하지 않습니다.


## 명시적인 macOS 공식 배포 단계

`scripts/official-mac-distribution.mjs`는 Mac arm64 Manager의 계획·서명·공증
단계를 제공합니다. 구현과 mock 검사는 실제 Developer ID/공증·설치 수락이
아닙니다. 기존 로컬 개발 서명 도구와 분리하며, 기존 자격 보유는 실행 승인으로
간주하지 않습니다. 현재 Node의 hardened-runtime 실제 실행은 미검증입니다.

계획 JSON에는 정확히 `version: 1`, 검토한 `sourceCommit`/`sourceTree`, 기존
Developer ID Application 인증서의 `identitySha1`(대문자40자리), `teamId`, 기존
`notaryProfile` 참조명, 새 절대경로 `workDir`, 실제 빌드될 `appPath`를 지정합니다.
암호·인증서 내용·API key·새 자격 등록은 입력하지 않습니다. source는 clean이어야
하며 workDir는 repository와 원본 app 밖의 새 경로여야 합니다. 먼저 실행 없는
계획을 검토하고 해당 산출물·기존 identity·Apple 제출을 승인합니다.

```sh
node scripts/official-mac-distribution.mjs plan /absolute/reviewed-plan.json
node scripts/build-desktop.mjs --plan --official-runtime-plan /absolute/reviewed-plan.json --ci -- --locked --offline --jobs 1
```

승인 후 공식 실행에는 별도 flag가 필요합니다. 준비는 기존 remote 준비 뒤,
최종 Cargo 컴파일 전에 고정 Node/Python Mach-O만 서명합니다. 원래 실행 파일과
두 manifest는 workDir에 먼저 보존합니다. Node에만 arm64의 `allow-jit` 후보를
적용하며 native/Python에는 blanket 예외를 추가하지 않습니다. 서명으로 바뀐
파일 hash·크기와 최종 manifests를 native에 포함하고 Tauri 중간 포장은 ad-hoc으로
유지합니다. 기존 private/unknown runtime 및 config overlay 거절은 유지합니다.

```sh
node scripts/build-desktop.mjs --official-runtime-plan /absolute/reviewed-plan.json --execute-official-distribution --ci -- --locked --offline --jobs 1
node scripts/official-mac-distribution.mjs finish /absolute/reviewed-plan.json --execute-official-distribution
node scripts/official-mac-distribution.mjs notarize /absolute/reviewed-plan.json --execute-official-distribution
```

finish는 같은 완료 빌드 호출이 고정한 app inventory와 최종 resource manifests의
바이트 결속을 확인하고 원본 app을 복사한 뒤 outer 서명·DMG 제작·readonly mount
동일성을 검사합니다. source app과 내부 resource를 사후에 바꾸지 않습니다.
모든 서명은 secure timestamp를 요청하므로 이 단계도 네트워크와 기존 자격
접근의 명시 승인 범위에 속합니다. get-task-allow 및 임의 Node 예외를 넣지 않습니다.

첫 notarize 호출은 최종 DMG를 제출하고 UUID를 저장합니다. 이후 같은 명령은
같은 UUID를 조회하며 Accepted/Invalid 결과의 로그를 보존합니다. Accepted 뒤에만
DMG staple/validate·Gatekeeper·최종 checksum을 기록합니다. 응답 불명확·동시 실행·
입력 변경은 자동 재전송하지 않습니다. `submission-uncertain`/`staple-uncertain` 또는
남은 lock은 실제 상태를 확인한 뒤 해당 작업만 조정해야 하며 무조건 삭제하거나
새 제출로 덮지 않습니다. 실패 부분 파일과 원래 입력은 보존합니다.

이 경로는 DMG ticket을 붙입니다. 별도의 app ticket 첨부나 준비 전 offline 설치,
clean-user 설치/업데이트, 실제 hardened Node/Python 동작을 통과로 표시하지 않습니다.
등록된 설치·실행·업데이트/오프라인 수락은 해당 산출물에서 별도로 확인합니다.

Node24.21의 [V8 Darwin allocator](https://github.com/nodejs/node/blob/v24.21.0/deps/v8/src/base/platform/platform-posix.cc)는 MAP_JIT를 사용합니다.
[upstream entitlement 파일](https://github.com/nodejs/node/blob/v24.21.0/tools/osx-entitlements.plist)은
디버깅 권한도 포함하므로 그대로 배포용으로 복사하지 않습니다.
[Apple 공증 조건](https://developer.apple.com/documentation/security/resolving-common-notarization-issues)과
[비동기 제출·로그 절차](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow)를 따릅니다.
