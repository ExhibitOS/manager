# Host profile 파일 사본과 비활성 추출

이 개발 CLI는 유효한 Manager profile의 선택 목록·이력과 모든 지원 host 파일을 AES-256-GCM 스트리밍 사본에 보존합니다. 불완전 후보, raw 작업 journal, 설정, 빈 파일도 포함합니다. 기존 `backup`/`backup-stream` 설정 사본과 payload 용도가 달라 서로 혼동해 복원할 수 없습니다. 기존 명령의 형식과 동작은 유지됩니다.

## 실행 조건

모든 Manager 버전·CLI·외부 파일 writer를 종료하고 해당 profile을 쓰는 서비스도 정지해야 합니다. `--host-writers-stopped`는 **운영자 확인**이며 CLI가 Docker·Podman·서비스 정지를 검증했다는 뜻이 아닙니다. 새 앱의 경로 anchor, 이전 앱의 session 잠금, profile와 등록 공간의 작업 잠금을 확보하지 못하면 사본을 거부합니다. 비협조적 writer를 격리하는 filesystem snapshot은 아닙니다.

profile은 현재 UID 소유의 canonical 절대 경로와 0700 폴더, 유효한 선택 목록·이력을 가져야 합니다. 손상된 선택 목록·이력의 salvage는 지원하지 않습니다. 사본·정확히 32바이트인 0600 일반 key 파일은 profile 밖의 canonical 0700 폴더에 따로 보관합니다. 키는 Git·로그·채팅에 넣지 않습니다. 새 archive 이름을 사용하며 기존 사본을 덮어쓰지 않습니다.

```sh
cargo build --release --locked -p exhibitos-lifecycle --bin exhibitos-profile
./target/release/exhibitos-profile --profile '<canonical original profile>' checkpoint-host \
  '<external private key>' '<new external archive>' --apps-closed --host-writers-stopped
./target/release/exhibitos-profile --profile '<same original canonical namespace>' extract-host \
  '<external private key>' '<existing archive>' '<new external recovery container>' --extract-only
```

추출 시 원래 profile 폴더가 없어도 부모 폴더는 존재해야 합니다. 사본은 **원래 canonical namespace**의 SHA256에 결합됩니다. 다른 profile 경로로 자동 재배치하지 않습니다. 대상 container는 아직 존재하지 않는 외부 경로여야 합니다.

## 보존과 실패 처리

UTF-8 상대 경로, 소유한 일반 파일·폴더와 POSIX 권한만 지원합니다. symlink·hardlink·special 파일, 외부 쓰기 권한·setid·읽기 불가 파일, 경로 escape·과도한 깊이·한도를 거부하며 조용히 건너뛰지 않습니다. 원래 profile과 알려진 등록 공간의 operation/session lock은 데이터가 아닌 잠금 인프라이므로 제외합니다. 부모의 경로 anchor도 사본 밖에 둡니다. 최대 데이터 64GiB(8MiB manifest 여유 제외), manifest 8MiB, 항목 100,000개, 경로 2,048 bytes·깊이32를 적용합니다. 전체 파일을 메모리에 올리지 않고 1MiB 인증 record와 제한된 manifest를 사용합니다.

백업은 파일별 SHA256·크기·inode·변경 시각·권한을 검사하고 마지막 inventory와 전체 암호문 인증을 재검사한 뒤 사본을 공개합니다. 추출은 전체 암호문 인증 후 private staging에서 manifest·파일별 hash·길이·선택 목록을 검사하고 새 container를 원자적인 no-replace rename으로 공개합니다. 추출 결과 `profile/`, `manifest.json`, `verified.json`과 인증된 plaintext `payload.pending`가 container 안에 남습니다. 실패 staging도 비공개 상태로 보존합니다. 키·원본·사본·실패 후보를 자동 삭제하지 않습니다.

사전 disk 여유 검사는 백업 예상 암호문, 추출 약2배 archive와 256MiB 여유를 요구합니다. 동시 disk 사용·filesystem overhead까지 예약하지는 않습니다. 중간 쓰기·동기화 실패는 성공으로 표시하지 않습니다. `HOST_WRITE_UNCERTAIN`에서는 pending/target의 실제 상태를 확인하고 기존 파일을 보존한 채 새 이름으로 재시도합니다. Mac/Linux atomic no-replace primitive를 사용하며 실제 Linux·Windows·전원 차단 검증은 별도입니다.

## 복원 범위와 남은 작업

Receipt와 manifest는 `externalVolumesSaved: false`, `hostWriterQuiescence: operator-acknowledged`를 명시합니다. Docker·Podman 외부 DB/blob 볼륨, ACL·xattr·원래 timestamp·gid는 보관하지 않습니다. 원본은 변경하지 않고 container와 staging은 0700으로 제한합니다. 이 파일 사본만으로 일관된 전시 데이터 복원 지점을 보장하지 않습니다.

`extract-host`는 복구 container를 만들 뿐 **설치 등록·원래 root 교체·runtime 활성화·writer 재개를 수행하지 않습니다**. Journal의 원래 절대 경로 결합이 유지되므로 추출 폴더를 임의로 실행 profile로 사용하면 안 됩니다. 외부 volume와의 일관된 체크포인트, canonical root를 가역적으로 교체하는 복구 intent·crash recovery, 실제 GUI·Windows·서명·전원 장애 검사는 후속 구현입니다. 전체 T08-02 완료나 운영 복원 완료로 해석하지 마세요.

## 개발 검사

```sh
cargo test --release --locked -p exhibitos-lifecycle --lib -- --test-threads=1
cargo clippy --release --locked -p exhibitos-lifecycle --all-targets -- -D warnings
python3 scripts/test-host-checkpoint.py --profile-cli '<built exhibitos-profile>'
```

새 비공개 합성 fixture만 사용합니다. source·키·archive·실패 staging·추출 파일을 보존하고 byte hash를 비교합니다. 실제 engine/GUI·Windows 검사와 구분해 결과를 기록합니다.
