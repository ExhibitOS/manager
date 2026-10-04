# 独立 Runtime 사본의 전체 인벤토리 관측

`inspect-target-inventory.py`는 완료된 `target-runtime-probe`의 정지된 synthetic DB/작품 사본만 사용합니다. 고유 project·volume label과 정지 상태를 확인하고, 유지보수 이미지의 PostgreSQL을 외부 네트워크 없이 실행합니다. 사본 DB의 repeatable-read/read-only transaction 및 maintenance advisory lock에서 공개 Platform inventory API를 호출합니다. 원본 볼륨은 마운트하지 않습니다. 사본의 PostgreSQL 정상 종료와 기존 container 상태 보존을 확인합니다.

```sh
python3 scripts/inspect-target-inventory.py --probe /absolute/private/target-probe-uuid --docker /absolute/docker
```

고유 private JSON은 DB identity, 전체 public schema fingerprint, migration checksums, table/sequence row counts/hashes, 모든 blob bytes/hashes, typed references 및 consistency issues를 보존합니다. 실제 row 값·password·key는 출력하지 않습니다. 새 helper·사본·진단 파일은 삭제하지 않습니다. Python -O는 금지하며 악의적인 daemon 응답의 메모리 한도를 완전히 보장하는 도구는 아닙니다.

2026-10-04 macOS ARM64 실제 관측: 50 table/sequence entries, 1 object, 0 consistency issues. 인증된 백업과 schema/migrations/blob/reference가 같고 49 table/sequence hashes가 같습니다. `auth_sessions`는 백업 0행에서 3행으로 변경됐습니다. 후보 및 신규·이전 Runtime 로그인 검사 이후 관측한 값이며, 로그인 전 사본의 전체 inventory를 취득하지 않았으므로 이 차이를 자동으로 허용하거나 정확한 신규 세션 귀속을 증명하지 않습니다. 전체 inventory equality는 통과하지 않았고, 다음 검사에서 초기 baseline과 단계별 write attribution이 필요합니다.

첫 helper는 pg CommonJS named import 오류로 PostgreSQL 시작 전 종료했고 보존됐습니다. 수정한 default import의 새 helper 실행은 종료0으로 관측을 완료했습니다. 이는 실제 등록 설치의 apply/migration/rollback, combined preflight/coherent security recovery, full corpus/Windows qualification을 대신하지 않습니다.

## 단계별 실제 비교

후속 probe는 완료 보고와 구분되는 `inventory-checkpoint.json`을 생성하고 `--checkpoint` 관측을 시작 전·target 로그인 후·previous 로그인 후에 실행합니다. 모든 서비스가 정지된 상태에서만 helper를 실행하며 중복 DB writer를 허용하지 않습니다. 각 관측의 private 경로·SHA-256은 최종 probe 보고에 보존합니다.

2026-10-04 신규 사본 실제 실행에서 세션 행은1→2→3이며 기존 각 행의 전체 JSON hash가 보존됐습니다. 두 전환 모두 identity/schema/migrations/49개 비세션 table·sequence 항목/blob/reference/issues가 동일했습니다. 실제 관측 schema fingerprint는 genuine target 선언과 일치했습니다. 이에 따라 이전 관측의 session 차이를 허용 목록으로 숨기지 않고 새 baseline에서 로그인 전후 변화를 대조했습니다. 이 fixture의 기본 로그인 흐름에 대한 증거이며 다양한 앱 writer·마이그레이션·전체 rollback 검증은 아닙니다.

실제 관측 JSON을 변형한 별도 비교 검사에서 schema·비세션 table·blob·기존 session 교체·추가 session·DB identity 변경6종이 거부됐습니다. 이 검사는 실제 DB 파괴/중단 주입을 대신하지 않습니다. Python compile/diff 검사도 통과했습니다. 모든 원본과 private 사본/helper는 보존합니다.
