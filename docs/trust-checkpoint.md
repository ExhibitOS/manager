# 영구 신뢰 저널 전체 사본 코어

`Store::checkpoint_trust(destination)`는 기존 profile anchor/trust lock을 보유한 Store에서 모든 committed generation을 새 private0700 디렉터리의0600 파일로 복사하고 fsync합니다. 초기 정책부터 최신 head까지 세대·이전 hash·scope·허용 전이·과거 operation/instance ID 예약을 다시 검증합니다. Pending 쓰기는 복사하지 않고 거부합니다. 최신 원본을 복사 전후 재관측하며 원본과 목적지 내용이 정확히 같을 때만 receipt를 반환합니다.

`Store::verify_trust_checkpoint(destination)`는 같은 최신 Store의 전체 이력과 사본의 모든 파일명을 대조합니다. 누락/추가/변형·권한/owner/link·alias·현재보다 오래된 head는 거부합니다. 최대4096개, 각96KiB이며 전체 records를 메모리에 보유하므로 최대 약384MiB가 필요할 수 있습니다. 프로필/활성 trust 안의 목적지는 거부하고 기존 경로는 덮어쓰지 않습니다. 실패한 부분 사본은 보존하며 성공 판정으로 사용하지 않습니다.

Receipt의 `fullHistoryVerified`는 최신 원본의 모든 기록 bytes를 확인했다는 뜻이며 `liveTrustRestored`는 항상 false입니다. 서명 개인키는 저널에 없지만 공개 정책/이력/경로 정보가 있으므로 이 사본은 비공개 plaintext 진단 자료이며 Git이나 public storage에 넣지 않습니다. 아래 암호화 CLI는 별도 비활성 추출까지 연결합니다. Offsite retention·원본 authority 손실 시 독립 복구/활성화·실제 recovery authorization은 아직 연결하지 않았습니다. 이 API는 floors/revocations를 낮추거나 과거 사본으로 원본을 교체하지 않습니다.

Store의 cooperative fences는 privileged 외부 writer를 완전히 배제하지 못합니다. 사본 검증 후의 외부 변경이나 whole-store rollback 저항을 보장하지 않습니다. Windows ACL 경계는 미검증이며 existing Store platform gating을 그대로 따릅니다. DB/blob/config/host checkpoint와의 단일 일관된 recovery point도 후속 통합 조건입니다.

실제 Rust filesystem 검사는 corruption, 공개 mode, destination reuse, profile-internal path, symlink alias, pending write, stale head, missing/extra generation과 Prepared intent/과거 reserved IDs 보존을 검사합니다. 이들 synthetic local tests는 실설치 복원·Windows·서명 release application qualification이 아닙니다.

## 인증 암호화 CLI와 비활성 추출

```sh
exhibitos-update archive-trust-checkpoint --profile /absolute/private/profile --installation default --key-file /absolute/private/key.bin --archive /absolute/private/recovery/trust.bin --apps-closed
exhibitos-update extract-trust-checkpoint --profile /absolute/private/profile --installation default --key-file /absolute/private/key.bin --archive /absolute/private/recovery/trust.bin --destination /absolute/private/recovery/inactive-trust --apps-closed
```

키는 별도 canonical regular single-link0600/current-owner 파일의 정확히32bytes이며 프로필 안에 두지 않습니다. 암호나 hex 문자열 파일이 아닙니다. CLI는 키를 로그에 출력하지 않고 완료/오류 반환 전에 입력 버퍼를 지웁니다. 키의 안전한 발급/보관과 compiler·AES 내부 메모리의 완전한 삭제는 별도 보안 경계입니다. 기존 파일/목적지와 프로필/활성 trust 내부 경로는 거부합니다.

AES-256-GCM의 random96bit nonce를 각 record에 사용합니다. Domain·완전한 header(scope/current generation/current head)·frame index를 AAD로 인증하여 frame 교체/재배치·오키·변형·truncation·추가 bytes를 거부합니다. Header4096bytes, record96KiB/4096개 한도이며 모든 plaintext는 최신 Store의 정확한 record bytes와 비교합니다. 암호문은 새 고유 private pending 파일에 fsync 후 no-replace rename으로 게시합니다.

추출은 고유 private pending 디렉터리에만 쓰고, 모든 frame 인증과 EOF·archive identity/timestamp·전체 원본 재검증을 마친 뒤 별도 비활성 경로로 no-replace 게시합니다. 실패한 암호문·부분 plaintext·키·진단 파일을 자동삭제하지 않습니다. 추출 사본에는 trust.lock이 없으며 앱에 활성화하지 않습니다. 최신 현재 Store가 반드시 있어야 하므로 원본 authority를 잃은 상황의 복구나 오래된 버전 재활성화 도구는 아닙니다. host/DB/blob/config와 단일 시점으로 통합하는 기능은 아직 남아 있습니다.

실제 genuine Prepared generation2 fixture CLI에서 암호화/별도 추출과 full-history 대조, 오키/암호문변형/공개 key mode/기존archive 거부, 원본 receipt/intent 보존5그룹을 확인했습니다. 테스트 전용 키와 실패 staging은 private ignored fixture에 보존합니다. 이 검사와 cryptographic filesystem 회귀는 Windows ACL·live recovery·release apply/rollback qualification을 대신하지 않습니다.
