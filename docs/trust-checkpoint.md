# 영구 신뢰 저널 전체 사본 코어

`Store::checkpoint_trust(destination)`는 기존 profile anchor/trust lock을 보유한 Store에서 모든 committed generation을 새 private0700 디렉터리의0600 파일로 복사하고 fsync합니다. 초기 정책부터 최신 head까지 세대·이전 hash·scope·허용 전이·과거 operation/instance ID 예약을 다시 검증합니다. Pending 쓰기는 복사하지 않고 거부합니다. 최신 원본을 복사 전후 재관측하며 원본과 목적지 내용이 정확히 같을 때만 receipt를 반환합니다.

`Store::verify_trust_checkpoint(destination)`는 같은 최신 Store의 전체 이력과 사본의 모든 파일명을 대조합니다. 누락/추가/변형·권한/owner/link·alias·현재보다 오래된 head는 거부합니다. 최대4096개, 각96KiB이며 전체 records를 메모리에 보유하므로 최대 약384MiB가 필요할 수 있습니다. 프로필/활성 trust 안의 목적지는 거부하고 기존 경로는 덮어쓰지 않습니다. 실패한 부분 사본은 보존하며 성공 판정으로 사용하지 않습니다.

Receipt의 `fullHistoryVerified`는 최신 원본의 모든 기록 bytes를 확인했다는 뜻이며 `liveTrustRestored`는 항상 false입니다. 서명 개인키는 저널에 없지만 공개 정책/이력/경로 정보가 있으므로 이 사본은 비공개 plaintext 진단 자료이며 Git이나 public storage에 넣지 않습니다. 암호화/offsite retention·CLI·독립 새 경로에서의 복원/활성화·실제 recovery authorization은 아직 연결하지 않았습니다. 이 API는 floors/revocations를 낮추거나 과거 사본으로 원본을 교체하지 않습니다.

Store의 cooperative fences는 privileged 외부 writer를 완전히 배제하지 못합니다. 사본 검증 후의 외부 변경이나 whole-store rollback 저항을 보장하지 않습니다. Windows ACL 경계는 미검증이며 existing Store platform gating을 그대로 따릅니다. DB/blob/config/host checkpoint와의 단일 일관된 recovery point도 후속 통합 조건입니다.

실제 Rust filesystem 검사는 corruption, 공개 mode, destination reuse, profile-internal path, symlink alias, pending write, stale head, missing/extra generation과 Prepared intent/과거 reserved IDs 보존을 검사합니다. 이들 synthetic local tests는 실설치 복원·Windows·서명 release application qualification이 아닙니다.
