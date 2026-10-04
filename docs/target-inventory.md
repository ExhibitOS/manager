# 独立 Runtime 사본의 전체 인벤토리 관측

`inspect-target-inventory.py`는 완료된 `target-runtime-probe`의 정지된 synthetic DB/작품 사본만 사용합니다. 고유 project·volume label과 정지 상태를 확인하고, 유지보수 이미지의 PostgreSQL을 외부 네트워크 없이 실행합니다. 사본 DB의 repeatable-read/read-only transaction 및 maintenance advisory lock에서 공개 Platform inventory API를 호출합니다. 원본 볼륨은 마운트하지 않습니다. 사본의 PostgreSQL 정상 종료와 기존 container 상태 보존을 확인합니다.

```sh
python3 scripts/inspect-target-inventory.py --probe /absolute/private/target-probe-uuid --docker /absolute/docker
```

고유 private JSON은 DB identity, 전체 public schema fingerprint, migration checksums, table/sequence row counts/hashes, 모든 blob bytes/hashes, typed references 및 consistency issues를 보존합니다. 실제 row 값·password·key는 출력하지 않습니다. 새 helper·사본·진단 파일은 삭제하지 않습니다. Python -O는 금지하며 악의적인 daemon 응답의 메모리 한도를 완전히 보장하는 도구는 아닙니다.

2026-10-04 macOS ARM64 실제 관측: 50 table/sequence entries, 1 object, 0 consistency issues. 인증된 백업과 schema/migrations/blob/reference가 같고 49 table/sequence hashes가 같습니다. `auth_sessions`는 백업 0행에서 3행으로 변경됐습니다. 후보 및 신규·이전 Runtime 로그인 검사 이후 관측한 값이며, 로그인 전 사본의 전체 inventory를 취득하지 않았으므로 이 차이를 자동으로 허용하거나 정확한 신규 세션 귀속을 증명하지 않습니다. 전체 inventory equality는 통과하지 않았고, 다음 검사에서 초기 baseline과 단계별 write attribution이 필요합니다.

첫 helper는 pg CommonJS named import 오류로 PostgreSQL 시작 전 종료했고 보존됐습니다. 수정한 default import의 새 helper 실행은 종료0으로 관측을 완료했습니다. 이는 실제 등록 설치의 apply/migration/rollback, combined preflight/coherent security recovery, full corpus/Windows qualification을 대신하지 않습니다.
