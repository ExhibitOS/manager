# Render 원격 사본 복원 연결 — 구현 후보

이 문서는 기존 T08-02/T10-04의 원격 DB 복원을 연결하는 **소스 후보**의 경계입니다. 네이티브 컴파일·새 앱 패키징·실제 Render/TLS/SQL·GUI 검사 결과가 아닙니다. 기존 `check-restore`는 계속 로컬 격리 사본 검사이며 이 기능의 원격 복원 결과로 바꾸지 않습니다.

기존 완료 백업 Job의 실제 `job.json` 해시, 백업 ID, 인증된 manifest 해시, 파일 수, 원래 Render 소유자/서비스/DB를 새 내부 복원 작업에 묶습니다. 원래 공개 service-backup 형식과 Deployment의 기존 여섯 공개 action은 바꾸지 않습니다. 추가 네이티브 action은 `remote-targets`, `remote-restore`, `remote-connect`만 받으며 renderer에서 URL·토큰·키·실행 파일·SQL·worker role을 받지 않습니다.

- 대상은 같은 원래 workspace의 다른 free PostgreSQL18/Frankfurt DB입니다. 신규 생성은 기존 quota/동일 operation 이름 재사용 guard를 사용합니다. 이미 활성 무료 DB가 있거나 상태가 불분명하면 생성하지 않습니다. 유효한 provider 만료 시각만 inactive 근거로 사용합니다. 원래 DB나 workspace를 삭제하거나 유료로 전환하지 않습니다.
- 신규 생성 결과가 `creating`이면 실제 ID를 같은 작업에 기록하고 멈춥니다. 이후 같은 작업의 재시도는 그 ID를 사용하며 생성 POST를 반복하지 않습니다.
- 기존 선택 대상은 확인된 좁은 CIDR이 없으면 SQL 전에 거부하며 접근 규칙을 수정하지 않습니다. 새 대상 한정 설정은 최초 자동 승인 검토에서 거부된 뒤 정확한 범위의 직접 사용자 승인을 받아 소스에 연결했습니다. 실제 POST 성공 후 admission을 통과한 응답에서만 호출되는 trusted `onPostgresCreated`가 작업/백업 Job/소유자/ID/이름에 묶어 durable creationProof를 남긴 새 DB의 빈 규칙에만 확인한 Mac IP 하나(/32 또는 /128)를 설정합니다. 동일 이름 GET 재사용이나 응답 손실 후 이름 조회만으로는 생성 증거를 발급하지 않습니다. 생성 증거 저장 실패도 성공으로 보지 않습니다. 바로 전 GET에서 다른 규칙이 생겼으면 덮어쓰지 않습니다. PATCH 전에 durable networkAttempted를 남기고 응답 손실·재시작 후에는 GET으로만 재조정합니다. 원본·기존 선택 DB·다른 규칙·전체 인터넷 허용은 변경하지 않습니다. 실제 원격 실행은 아직 하지 않았습니다.
- 고정된 해시 검증 helper entry만 원래 archive를 읽기 전용 mount로 받습니다. 기존 f28 Platform worker가 인증한 manifest/원래 public authority를 확인한 뒤, 내부 callback으로만 `verified-remote-restore-target` 역할을 구성합니다. TLS verify-full, 실제 물리 DB 구별, 실제 SQL/blob 빈 상태, 원래 전체 inventory/sequence/blob/migration/서명 권한 확인은 기존 worker 계약을 사용합니다. 복원된 manifest도 원래 해시와 다시 비교합니다.
- 원래 백업 키는 기존 per-job Keychain에서만 읽습니다. 새 키를 만들거나 불러오기 실패를 현재 설정으로 대체하지 않습니다. helper 작업량은 기존 저장 공간 budget과 32GiB 상한, 시작 시 가용 공간 및 실행 중 owned work-volume 사용량으로 제한합니다. 초과하면 그 helper 자체만 실패 종료하며 원본이나 외부 작업에 정리 명령을 보내지 않습니다.
- import가 시작된 뒤 결과가 없으면 부분 원격 SQL이 있을 수 있어 자동 재import하지 않습니다. native `dispatched` 및 runner `importing` 경계를 남기고 후보를 보존합니다. SQL 전에 고정 runner가 남긴 `admitting` 기록만 같은 작업의 재시도를 허용합니다.

## 명시 서비스 연결과 복구

검증된 대상의 실제 DB 물리 식별 해시가 있는 작업만 연결할 수 있습니다. 원래 서비스는 실제 일시 중지 상태, free Node/repo/소유자, auto-deploy off, 원래 알려진 설정·DB URL을 확인합니다. 현재 관측한 provider live deploy ID/commit과 서비스 직접 env 전체를 작업별 add-only Keychain 항목에 보존합니다. 이 값은 renderer나 plaintext journal에 저장하지 않습니다. **현재 관측한 commit을 백업 당시 runtime 증거로 소급하지 않습니다.**

사용자가 다른 쓰기 중지 및 연결·배포·재개를 명시 확인하면, 바로 전 원래 env를 재조회하여 비교하고 `DATABASE_URL` 하나만 교체한 전체 env를 PUT합니다. PUT만으로 배포되었다고 주장하지 않습니다. 실제 서비스 resume 및 관측한 정확한 commit의 deploy를 요청하고, 해당 deploy ID/commit/live 상태, 대상 물리 DB, readiness, 기존 계정과 필요 시 원래 정적 publication/revision을 확인합니다. 로그인은 원래 DB 데이터 복원 검증 뒤에만 실행합니다.

실패·응답 손실 시 같은 작업의 durable checkpoint를 읽습니다. 이미 알려진 배포 ID는 읽기 관측으로 재조정하며 무작정 새 deploy를 만들지 않습니다. 복구는 service를 일시 중지하고, env가 정확한 원래 값 또는 이 작업이 쓴 값일 때만 원래 env로 되돌리고, 정확한 보존 deploy로 provider rollback을 요청합니다. 다른 사용자의 env 수정은 덮어쓰지 않습니다. rollback 관측 이후 완료 직전에도 원래 env를 다시 비교하며 달라졌다면 복구 완료를 기록하지 않습니다. rollback 응답이 사라지면 다시 rollback POST를 보내지 않습니다. `connection-restored`는 원래 env/관측 commit/일시 중지 복구 기록이며, 만료된 원래 DB가 다시 건강하게 실행됨을 뜻하지 않습니다.

## Producer / consumer와 남은 실제 확인

`prepare-remote-runtime.mjs`는 고정 Node24.21.0 archive, 정확한 Deploymente4379bfe 소스, 세 개의 새 고정 script를 기존 resource manifest에 해시로 묶습니다. `remote.rs`와 `remote_restore_pins`가 같은 목록과 compile-time manifest를 검증합니다. 기존 패키지에는 이 파일들이 없으므로 정확히 unavailable이며 PATH fallback이나 임의 entry mount는 없습니다. Platform f28의 git source 계약은 확인했지만 기존 실제 maintenance image 안의 해당 모듈/권한/의존성 실행은 이 소스 작업에서 검증하지 않았습니다.

실행한 검사는 작은 synthetic Node 회귀와 Rust/TS 구문 검사뿐입니다. 실제 이미지/패키지 재생성, Cargo/typecheck/GUI, provider 계정 quota/DB 권한, 실제 TLS·empty SQL·restore/inventory/authority, 실제 deploy/rollback 및 원격 중단 재조정은 총괄의 별도 통합 검증 대상입니다. 미완료 상태를 원격 복원 완료나 출시 완료로 세지 않습니다.
