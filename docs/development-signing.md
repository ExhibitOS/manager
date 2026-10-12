# 기존 macOS 앱의 개발 서명·DMG 제작

T08-03의 로컬 개발 서명 경로입니다. 이미 빌드하고 source version·포함 파일을 검증한 Manager 앱을 새 비공개 경로에 복사하고, **현재 Mac의 기존 Apple Development 인증서**로 App과 DMG를 각각 서명합니다. 원본 앱과 포함된 Node·runtime 자료를 보존합니다. `build-installer.mjs`의 미서명 prototype 경로는 그대로 유지합니다.

Apple Development 서명은 로컬 개발 검사용입니다. Developer ID 배포 서명·공증·stapling·Gatekeeper 승인이나 Windows Authenticode를 대신하지 않습니다. 이 명령은 앱 실행·설치·제거·업데이트·offline Runtime 검사를 수행하지 않습니다. 해당 실제 완료 조건은 별도로 검증합니다.

## 입력 준비

검토한 깨끗한 Manager checkout, 기존 App 빌드 및 포함 자료 검증 영수증, 그 App 전체 파일 목록이 필요합니다. 목록은 App 상대 경로를 키로 하고 각 파일의 `sha256`, `bytes`, `mode`를 기록한 JSON 객체입니다. 경로를 추측하거나 임의의 새 목록으로 빌드 근거를 대체하지 않습니다. 실제 빌드 영수증에 연결된 목록과 그 SHA256을 검토하고 사용하세요. `--source-commit`은 현재 파이프라인 checkout HEAD, `--build-source-commit`은 원본 App의 실제 빌드 커밋입니다. 두 Git tree에서 `docs/**`, `README.md`, `AGENTS.md`, 이 서명 스크립트만 제외한 모든 tracked blob·mode가 같아야 합니다. product 코드·자산·vendor·라이선스·구성·lockfile 또는 다른 빌드 스크립트가 바뀌면 기존 App 사용을 거부합니다. 두 커밋과 비교한 목록 hash를 별도로 기록하므로 기존 App을 새로 빌드한 것으로 표시하지 않습니다. 이 스크립트가 확인하는 source commit/tree와 App 파일 목록의 일치는 **기존 빌드의 source→App 연결 영수증을 대체하지 않습니다**.

Xcode에서 본인 Apple 계정의 기존 Apple Development 인증서를 준비합니다. 운영자가 Keychain Access에서 선택한 해당 인증서의 SHA1을 명시적으로 전달합니다. 스크립트는 인증서·키를 생성하거나 내보내지 않고, Apple Development 유형이 아닌 identity를 거부합니다. 암호·키는 명령행이나 Git에 넣지 않습니다. 인증서 이름이 포함된 검증 도구 출력은 새 비공개 출력 폴더에만 저장합니다.

## 실행

아래 변수를 실제 검토한 입력으로 지정합니다. 모든 입력·출력 경로는 절대 경로이며 symlink를 거부합니다. `OUTPUT`은 checkout과 원본 App 밖의 **존재하지 않는 새 폴더**여야 합니다. 기존 출력은 덮어쓰지 않습니다.

```sh
python3 scripts/sign-development-installer.py \
  --app "$REVIEWED_APP" \
  --inventory "$REVIEWED_APP_INVENTORY" \
  --inventory-sha256 "$REVIEWED_INVENTORY_SHA256" \
  --source-commit "$REVIEWED_PIPELINE_COMMIT" \
  --build-source-commit "$REVIEWED_APP_BUILD_COMMIT" \
  --identity-sha1 "$APPLE_DEVELOPMENT_CERTIFICATE_SHA1" \
  --output "$OUTPUT" \
  --execute-development-signing
```

입력 App 최대 180MiB, DMG 최대 300MiB, 해당 실행의 총 저장 공간 증가 최대 1GiB를 적용합니다. 시작 시 최대 증가량·다른 작업 192MiB 여유·최소 6GiB 잔여 공간을 합한 공간을 요구합니다. 총괄 실행 관리자는 다른 무거운 검사와 병행하지 않고 전체 workspace의 저장 공간 입장 검사를 별도로 수행합니다. 작업은 270초, 개별 출력은 1MiB로 제한하고 자식 프로세스 그룹을 종료합니다. 실패 시 기존 입력과 새 부분 산출물을 보존합니다.

## 결과 확인

`result.json`은 source commit/tree, 입력 목록 hash, 산출물 크기·SHA256, 실제 각 명령 종료 코드와 서명 범위를 기록합니다. `signed-inventory.json`은 서명 후 App의 파일 목록입니다. App과 DMG 서명을 별도로 확인하며, readonly mount의 App 목록·strict deep signature가 일치해야 성공합니다. `--deep`은 검증에만 사용하고 포함 vendor 실행 파일을 다시 서명하지 않습니다. 원본 App·Resources·입력 목록·source와 DMG 바이트를 마지막에 다시 확인합니다.

이미지는 자동으로 앱을 열지 않고 새 전용 mount 경로에 붙입니다. 성공·실패 모두 해당 이미지의 attachment만 분리하며, attachment 정보를 읽을 수 없는 경우에도 정확히 새 DMG 경로와 연결된 장치만 처리합니다. 분리 실패는 성공으로 표시하지 않습니다. 기존 App·사용자 데이터·출력 파일을 삭제하거나 글로벌 Applications 폴더에 설치하지 않습니다.

성공 영수증에서도 `productionSigned`, `notarized`, `gatekeeperQualified`, `nativeGUIExecuted`는 모두 `false`입니다. 개발 설치 파일과 영수증은 검증한 별도 보존 위치에 복사하고 hash를 확인합니다. 이를 production 다운로드로 공개하지 않습니다. 배포 서명·공증 절차와 나머지 실제 설치 검사는 [설치 프로그램 문서](installer-prototype.md)를 따릅니다.
