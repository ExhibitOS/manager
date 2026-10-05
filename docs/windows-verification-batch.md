# Windows 한 번에 검증하기

이 문서는 T08-01(수명주기), T08-02(백업·업데이트), T08-03(설치 프로그램)의 Windows 검사를 하나의 순서로 모은다. **현재 전체 배치는 준비 중이며 사용자에게 실행을 요청하지 않는다.** Windows 전용 구현과 안전한 합성 fixture가 준비되면 최종 소스 commit과 명령을 한 번만 제공한다. 소스 변경으로 영향을 받는 검사만 다시 수행한다.

## 이미 확인한 항목

Windows11 x64 / Docker Linux engine29.8.1 / Compose5.5.1, Node24.21.0/npm11.19.0, Rust1.99 MSVC 및 Build Tools가 준비됐다. Managera42be3c 전체 코어·통합·데스크톱121검사0실패4ignored, 관리 공간 생성·전환·자동 재열기 선택 유지, Platforme6df4bc 라이선스 해시·Git2검사·실제amd64 bundle 생성이 확인됐다. 실제 GUI 설치·시작 readiness·브라우저·API 연결·정지 notready·재시작 ready와 Manager9ba2100 시작/정지/재시작의 즉시 진행→완료 표시도 사용자 관찰로 확인했다. 같은 결과를 무조건 반복 요청하지 않는다. 최신 GUI binary hash/build log는 별도로 수집되지 않았으므로 아래 읽기 전용 기록 수집으로 보완한다. 과거4ignored Engine 검사는 통과로 세지 않는다.

## 최종 한 번의 실행 순서

|순서/ID|담당 task|검사·완료 기준|현재 준비 상태|
|---|---|---|---|
|0/WIN-00|T08-01|최종 commit과 build log, 앱·manifest·Runtime archive SHA256/크기 기록. 기존 generated 변경 보존|읽기 전용 도구 준비; 최종 통합 commit 고정 대기|
|1/WIN-01|T08-01|앱 종료→같은 공간 재열기→선택·설치 기록 유지. 정지 상태에서 HTTP 접속 중단을 별도 확인|기존 fixture 사용 가능; 배치 실행 보류|
|2/WIN-02|T08-01|새 합성 공간의 port 충돌·실행 도구 부재/정지·권한 거부·명시적 retry에서 안전한 코드와 원본 보존 확인|native 자식 프로세스 오류 fixture 준비; 실제 Engine 장애·retry integration은 준비 중|
|3/WIN-03|T08-01|Podman 또는 Docker Desktop 외 지원 adapter의 동일 설치·실행 경로 확인|대체 engine qualification 준비 필요; 지금 추가 설치 요청 없음|
|4/WIN-04|T08-02|새 synthetic corpus DB/blob/config/image/profile 백업, 별도 공간 복원 후 로그인·작품·전시·서명 설정·해시 일치|Windows 성공 경로의 platform gates/host integration 미완료; 실행 금지|
|5/WIN-05|T08-02|백업·복원 interruption/cancel/retry/helper 진단에서 원본·후보·키 보존, 성공 오표시 없음|WIN-04 prerequisite 구현 및 실제 fixture 준비 필요|
|6/WIN-06|T08-02|서명된 개발 update, 호환성·disk·pre-update backup, migration/health 실패의 전체 data rollback, trust floors/revocations 유지|Windows key/policy/host/update gate 미완료; 실행 금지|
|7/WIN-07|T08-03|깨끗한 별도 사용자/VM 설치·제거·업데이트, checksum/signature/offline local start, 설정/데이터 보존|installer/signing/release artifact와 복원점 준비 필요|
|8/WIN-08|T08-01/02/03|작업 중 앱 종료·OS/Engine 재시작·저장 durability/재열기 시 불확실한 작업을 완료로 오인하지 않음|VM snapshot과 복원 검증, failure corpus 준비 필요; 실제 사용자 PC 전원 중단 금지|

전체 배치는 로컬 빌드/네트워크 속도·데이터 크기에 따라 소요 시간이 달라진다. 최종 안내 때 자동 명령 구간·사용자 클릭 구간·다운로드와 재부팅 구간의 예상 시간을 따로 산정한다. 현 단계에서 전체 예상 시간을 확정하지 않는다. 요청을 보류해도 완료 기준은 축소하지 않는다.

## 읽기 전용 기록 도구

현재 저장소에서 다음 예시를 실행하면 stdout에 JSON을 출력한다. 새 설치·서비스 시작·파일 삭제·ACL 변경·Git checkout·환경변수 변경은 수행하지 않는다. 공유 출력에는 키/credential/config 내용과 절대 경로를 넣지 않는다. Windows 실행은 최종 배치에 포함하며 지금 추가 실행을 요청하지 않는다.

```powershell
node scripts/verification-receipt.mjs --binary .\target\release\exhibitos-manager-desktop.exe --bundle "<선택한 새 검사 공간>\bundle"
```

Git source commit/기존 tracked 변경 여부, binary/manifest/hash-bound tar 크기·SHA256을 기록한다. manifest의 경로탈출·archive byte/hash 불일치는 거부한다. **앱 binary가 해당 소스로 빌드됐다는 증명·서명 검증·설치 권한·readiness 판정 도구가 아니다.** 실제 build log, 실행 결과와 사용자 관찰을 함께 연결해야 한다. 기존 API 연결 확인과 단위 검사만으로 전체 복원·업데이트·durability를 통과 처리하지 않는다.

## 결과 전달과 안전한 중단

최종 batch ID/sourceCommit과 각 WIN-ID의 `passed/failed/not_run/blocked`, 실행 시각·명령 종료 코드·safe code·GUI 관찰을 함께 기록한다. 오류가 나면 그 단계에서 멈추고 실패 후보를 보존하며 에이전트가 다음 조치를 준비한다. 암호·token·키·runtime.env·raw container 환경·실제 작품을 채팅이나 Git에 보낼 필요는 없다. 설치 재실행/reset/prune/volume 삭제로 검사를 성공처럼 만들지 않는다. 모든 파괴 시나리오는 새 synthetic 환경 및 검증한 복원점에서만 실행한다.

## WIN-02 native 오류 부분 검사

최종 배치에서 다음 명령을 포함한다. 현재 사용자의 개별 실행은 요청하지 않는다. 테스트는 새 임시 경로에서 작은 Rust 합성 실행 파일 하나를 컴파일해 실제 자식 프로세스로 실행한다. 권한/포트/일반 실패의 safe code, stderr 미노출, 성공 stdout 분리, timeout과 도구 부재 구분을 검사한다. Docker/Podman을 정지하거나 기존 데이터·ACL·환경 설정을 바꾸지 않는다. 합성 child 결과는 실제 Docker/Podman 장애나 전체 retry 성공 증거가 아니다. fixture 파일은 후속 확인을 위해 보존하며 자동 삭제하지 않는다.

```powershell
cargo test -p exhibitos-lifecycle --lib engine_failure_native --locked
```


## WIN-06 정책·서명 문서 읽기 선행 구현

Windows `exhibitos-update`의 작은 정책/서명 문서 입력은 기존 NTFS public reader에 연결한다. 모든 조상 경로/파일 식별자와 ACL을 검사하고 읽는 동안 native handle을 유지해 writer/delete 공유를 거부한다. 기존 ACL을 수정하거나 파일을 private로 채택하지 않는다. byte quota와 읽기 전후 변경 검사를 유지한다. 반환된 bytes는 입력일 뿐이며 기존 Ed25519·policy·expiry·replay 검증이 별도로 필요하다.

최종 일괄 검사에 다음 세 검사를 포함한다. 새 합성 폴더만 생성하며 실패 후보와 파일은 남긴다. 정상 문서 정확 읽기, 열린 writer/hardlink/초과 크기 거부, 비파일·누락·무제한 입력 거부를 확인한다. 아직 Windows에서 실행하지 않았다.

```powershell
cargo test -p exhibitos-lifecycle --bin exhibitos-update windows_update_inputs --locked
```

큰 artifact reader·외부 private key reader·private staging API는 후속 절의 소스로 구현됐다. Native 검사는 미실행이고, host checkpoint/trust journal 및 full update/rollback은 아직 미완료다. 작은 입력 reader 구현으로 WIN-06 전체를 준비 완료 또는 통과 처리하지 않는다.


## WIN-06 큰 artifact 읽기 선행 구현

Windows `verify`의 artifact 입력을 실제 파일/조상 경로가 고정된 NTFS streaming reader에 연결한다. 16MiB 문서 제한과 달리 signed manifest의 정확한 byte 수와 SHA256을 streaming으로 확인한다. 열린 writer, hardlink/reparse, 파일 교체·이름 변경을 허용하지 않고 매 read 및 마지막 검사에서 ACL/식별자/크기·수정 시각을 확인한다. 실패한 재검사는 이전 artifact proof를 지운다. 원본 파일/ACL은 변경하지 않는다.

```powershell
cargo test -p exhibitos-lifecycle --lib windows_artifact_inputs --locked
```

새 합성 파일을 사용하는 4개 native 검사는 최종 배치에 포함하고 지금은 실행하지 않는다. 17MiB 초과 성공, busy writer/hardlink/size/hash/expiry/name/missing 실패와 원본 보존을 확인한다. synthetic verification fixture는 서명 검증 증거가 아니며, production CLI는 기존 실제 서명 검증 이후 이 reader를 호출한다. retained private staging과 외부 key reader의 후속 구현은 아래 절에 기록한다. Host/trust checkpoint, 실행자 연결과 full rollback은 아직 미완료다. Reader 검사만으로 import/activation 완료를 주장하지 않는다.


## WIN-06 외부 checkpoint key 선행 구현

Windows update CLI는 기존 owner-only32byte key를 NTFS guarded reader로 읽는다. 원본 profile의 native directory ID와 key 부모/조상 ID를 비교해 내부 key 및 대소문자 alias를 거부한다. source profile이 없는 복구 namespace도 안전한 부모 경로를 고정하고 부재를 읽기 전후 확인한다. 키·부모 ACL을 수정하거나 새로운 key/profile을 만들지 않는다. 원본 read handle과 parent guards는 검사 종료까지 유지하고 실패 시 반환 후보 key array를 지운다.

```powershell
cargo test -p exhibitos-lifecycle --lib windows_external_key --locked
```

새 합성 fixture4검사는 최종 단일 배치에 포함하며 지금 개별 실행을 요청하지 않는다. 정확한32bytes·existing/missing namespace, inside/nested/case alias, busy writer/hardlink/length, Everyone read grant refusal와 원본 보존을 확인한다. ACL grant 변경은 새 합성 key에만 적용한다. 이 reader만으로 Windows host/trust/staging/full restore/update를 실행 가능 또는 PASS로 처리하지 않는다.


## WIN-06 Windows private staging 선행 구현

`artifact::stage`는 기존 보호된 NTFS 부모를 검사하고 새 owner-only 후보 폴더·파일을 만든다. 원본 public read guard를 복사/검증 종료까지 유지한다. signed exact byte 수·SHA256을 다시 확인하고, 쓰기 핸들을 닫은 뒤 READ 전용·FILE_SHARE_READ 핸들과 private directory/ancestor guards를 반환 객체의 수명 동안 보관한다. 반환 객체가 보관된 동안 새 writer/delete/rename을 거부하고 재검사와 실행자 입력 clone 전에 ACL/native identity/length/mtime를 확인한다. 원본이나 기존 ACL을 채택·수정하지 않는다. 실패 후보는 진단용으로 남긴다. 기존2GiB artifact cap과 artifact+2GiB 여유 기준을 유지한다.

```powershell
cargo test -p exhibitos-lifecycle --lib windows_artifact_staging --locked
```

4개 합성 native 검사: exact/read-only retained copy 및 원본 보존, writer/hardlink refusal, hash 실패 후보 보존·quota/expiry, 새 후보에 대한 Everyone read grant 거부. 현재 MSVC 교차 컴파일만 확인했으며 Windows 실제 실행은 최종 단일 배치까지 미실행이다. 정상 검사 종료 후 새 전용 fixture만 정리하고 실패 시 자료를 보존한다. Native byte sharing/ACL은 power-loss durability 증거가 아니다. Store/host/trust/OCI 실행 연결 및 fresh full recovery/update/rollback은 별도 선행 작업이며 이 API 구현만으로 실행하지 않는다.

## WIN-06 Windows 불변 trust 기록 발행 선행 구현

NTFS private 폴더에서 새 `pending-UUID.json`에 기록하고 sync한 뒤, 보관된 DELETE 권한 핸들을 사용해 `ReplaceIfExists=false`로 새 세대 이름을 발행한다. 원본 파일 식별자와 ACL을 전후 확인하고, 쓰기 핸들을 닫은 뒤 읽기 전용 shared-lock guard로 정확한 bytes를 다시 읽는다. 기존 이름·대소문자 alias 충돌은 덮어쓰지 않으며 실패 후보를 보존한다. 발행 후 검사 실패는 완료가 아닌 uncertain으로 보고한다. Trust writer는96KiB 기록 한도를 유지하며 이 helper에 연결한다.

```powershell
cargo test -p exhibitos-lifecycle --lib windows_generation_publication --locked
```

4개 합성 검사는 정확한 bytes 재열기·read handle의 writer/rename 거부, 기존 이름·case alias 보존 및 pending 유지, 잘못된 이름/한도 거부, 동시 발행2개 중 정확히1개 성공을 확인한다. Native 실행은 최종 단일 배치까지 미실행이다. 생성된 작은 전용 fixture만 정상 검사 후 정리하고 실패 시 보존한다.

이 helper의 file sync는 directory/power-loss durability 증거가 아니다. Windows `Store::provision/open`은 전체 root identity·case-safe scope·읽기·lock·directory publication/durability 연결이 아직 없어 platform gate를 유지한다. 따라서 실제 Windows trust 업데이트·checkpoint·전체 백업/복원은 이 변경만으로 실행 가능하거나 통과한 상태가 아니다.


## WIN-06 Windows trust root·읽기·잠금 선행 구현

Store 소스에는 보호된 NTFS root 가드를 저장소 전체 수명 동안 유지하는 adapter를 연결했다. 부모 native directory ID와 대소문자를 통일한 ASCII profile leaf 이름으로 논리 scope를 계산하며, 같은 부모의 case alias와 missing/replacement profile은 같은 scope를 사용한다. 다른 installation UUID는 별도 scope를 쓴다. 새로운 원본/부모 namespace로의 trust 이식 권한을 부여하지 않는다.

기록 읽기는 owner-only ACL·native file identity·single-link를 검사하는 bound read handle로96KiB 이하 bytes를 읽고 빈 committed 기록을 거부한다. trust.lock은 기존 bytes/ACL을 바꾸지 않고 exclusive native lock으로 연다. pending-UUID 기록은 bounded private input으로 검사하고 부분/빈 bytes가 있어도 committed history나 floor로 채택하지 않는다. Root ACL/경로 변화, writer/hardlink/초과 크기, 중복 lock은 거부한다.

```powershell
cargo test -p exhibitos-lifecycle --lib windows_trust_root --locked
```

5개 새 합성 native 검사는 scope case alias/부재 보존·installation 구분, exclusive lock/root rename 거부, writer/hardlink/빈/초과 기록, 새 root에 대한 Everyone read ACL grant 거부, partial pending 보존을 확인한다. MSVC 교차 컴파일만 통과했으며 실제 Windows 검사는 단일 최종 배치까지 미실행이다.

전체 Store 플랫폼 gate는 유지한다. [Microsoft Directory Handles](https://learn.microsoft.com/en-us/windows/win32/fileio/obtaining-a-handle-to-a-directory)와 [FlushFileBuffers](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers)의 공식 문서는 현재 구현에 필요한 directory publication/power-loss durability 보장을 증명하지 않는다. File sync나 이 adapter의 guard를 그 증거로 대체하지 않는다. 남은 directory durability·host/archive·authority-loss/full recovery·executor 연결 및 crash corpus가 필요하다.
