# 웹 전시 공개 — Pages와 선택적 Render

1차 hosted 경로는 승인한 정적 전시를 Cloudflare Pages에 올립니다. API 준비를 고르면 같은 정적 전시를 먼저 공개하고, 별도로 Render의 무료 API·PostgreSQL 자원을 준비합니다. 기존 서버 도구는 접힌 보조 영역에 보존하며 이 흐름의 필수 조건이 아닙니다.

Mac 앱에서 공개 방식 선택 → 제공자 API 토큰을 암호 입력란으로 입력 → 키체인 보관·계정 접근 확인 → 계정/워크스페이스와 선택적 API·DB 목록 선택 → 승인한 전시 묶음 선택 → 공개 동의 → 배포 순서입니다. 계정·서비스 식별자를 직접 입력하지 않습니다. 전시 제목·공개 식별자·프로젝트 이름은 native가 공개 묶음에서 확인합니다. 원본 촬영이나 임의 폴더는 배포 입력으로 받지 않습니다. 전체 파일·해시는 동봉된 배포 어댑터가 네트워크 업로드 전에 확인합니다.

토큰은 macOS Security.framework의 Keychain에 제공자별로 보관합니다. renderer의 password 값은 제출 직후 지우고, 명령행·환경변수·로그·브라우저 저장소에 쓰지 않습니다. 동봉 Node에는 native가 stdin으로만 전달하며 고정된 공개 결과만 반환합니다. 공식 가입·토큰 설정 링크와 실제 제공자 결과로 확인한 공개 URL만 OS 브라우저로 엽니다. 연결 해제는 해당 토큰만 제거하고 기존 전시·서비스·데이터·복구용 런타임 설정을 삭제하지 않습니다.

선택적 새 API의 tenant/owner bootstrap·서명 키는 native에서 생성하고 workspace/project별 Keychain에 보존합니다. 이는 새 빈 원격 DB 준비이며 로컬 Studio/CMS의 계정·작품 이전 완료를 의미하지 않습니다. 공개 publication ID·revision·index 해시는 선택한 공개 묶음 그대로 전달합니다. Render가 제공한 `expiresAt`만 표시하고 임의 만료일을 만들지 않습니다. 제공자의 `deployed`, API 응답 건강, 선택 전시의 기능 검증은 서로 구분합니다. 공개 호스트에서 실제 정적·API·데이터·복구가 확인되기 전 동적 전시 전체 성공으로 표시하지 않습니다.

쓰기 전에 secret-free target/source manifest pin과 작업 intent를 같은 native Keychain의 별도 작업 기록에 보존하고, 결과에는 양쪽 제공자의 안전한 ID·상태·URL만 기록합니다. 앱 재시작은 자동 POST를 하지 않습니다. 이전 작업은 표시하고 사용자가 ‘이전 배포 상태 확인’을 누르면 기존 이름·content hash·ID로 GET-only 조정합니다. 원래 묶음이 없거나 달라지면 조회·기존 URL은 유지하면서 새 업로드만 막습니다. 불확실한 생성은 완료나 무변경으로 가정하지 않으며, 새 Render 생성 전에 실제 자원 목록을 확인합니다. 자동 삭제는 없습니다.

검증은 TypeScript·lint·native offline check, 합성 응답의 provider/URL/비밀 경계 단위 검사, 브라우저의 정적 공개·입력 지우기·재시작 GET-only 흐름으로 구분합니다. 실제 제공자 토큰·계정·자원 생성·공개 URL·서명 앱의 UI는 실행한 증거 없이 통과로 주장하지 않습니다. 개발용 브라우저의 native bridge 모사는 실제 Keychain이나 클라우드 배포 증거가 아닙니다.

디자인 적용: P1/P3/P5·PD1 — 필요한 제공자만 연결하고, 공개 제목과 다음 행동을 먼저 보이며 내부 식별자는 상세에 둡니다. 화면 모드는 기존 system/light/dark 설정과 semantic 색·간격을 그대로 사용합니다.
