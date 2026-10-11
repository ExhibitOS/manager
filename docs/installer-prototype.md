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

산출물은 Windows `target/release/bundle/nsis`, macOS `target/release/bundle/dmg`에 생성한다. 영수증은 실제 각 설치 파일의 이름·크기·SHA256과 source commit/tree, version 및 입력 파일 hash를 기록한다. 일반 경로·비밀번호·키 내용은 영수증에 넣지 않는다. source와 도구·파일 바이트가 제작 도중 바뀌면 성공 영수증을 내지 않는다. symlink/hardlink 설치 파일과 기존 출력도 거부한다.

깨끗한 source에만 실행한다. Windows의 기존 자동 생성 permission 파일에 `M` 표시가 있으면 보존하고 별도 checkout을 사용한다. 기존 사용자 변경을 reset하거나 현재 앱·복구 자료를 삭제해 실행 조건을 맞추지 않는다. 실패 시 부분 출력과 로그를 보존하고 다음 시도는 별도 검토한 깨끗한 checkout에서 수행한다.

결과의 `distribution`은 `unsigned-prototype`, `productionSigned`는 `false`, `signatureVerification`은 `not_run`이다. 이는 배포 서명 증거가 없다는 뜻이며 macOS 도구가 만드는 로컬 ad-hoc code signature를 배포용 신원 검증으로 간주하지 않는다. 실제 NSIS/DMG 제작·설치·제거·업데이트와 offline local start는 각각 해당 운영체제에서 별도로 확인해야 한다. 합성 Node 검사는 이 결과를 대신하지 않는다.

## 서명·공증과 배포

현재 prototype 명령에는 운영 서명 키를 입력하지 않는다. 서명 계정이 준비되면 별도 승인된 배포 절차에서 검토한 동일 source version을 패키징하고 Windows Authenticode 검증 또는 macOS Developer ID 서명·notarization·stapling·Gatekeeper 검사를 수행한다. 계정·인증서·키는 OS/비밀 저장소에 보관하며 Git과 영수증에 넣지 않는다. 서명 완료 파일의 SHA256을 다시 계산하고 실제 검증 결과와 source version을 함께 연결한다.

실제 signed 결과와 clean install/uninstall/update, offline Runtime 시작 증거가 갖춰지기 전에는 production 다운로드나 검증 완료 문구를 게시하지 않는다. 현 단계의 다운로드 안내는 검토된 개발 산출물에만 적용한다. 설치 후 전시 서버 준비 방법은 [데스크톱 사용법](desktop.md), Windows의 남은 실제 검사는 [검증 배치](windows-verification-batch.md)를 따른다. Hosted CI와 유료 서명 서비스를 이 명령으로 활성화하지 않는다.
