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

백업은 파일별 SHA256·크기·inode·변경 시각·권한을 검사하고 마지막 inventory와 전체 암호문 인증을 재검사한 뒤 사본을 공개합니다. 추출은 각 인증된 chunk를 새 private staging의 파일에 직접 기록합니다. 전체 암호문을 풀어 놓는 `payload.pending` 중간 사본은 만들지 않습니다. manifest·파일별 hash·길이·선택 목록, 마지막 인증 frame와 EOF, 파일 권한·receipt 기록, 원래 encrypted archive와 external key 재검사를 모두 통과한 뒤 새 container를 원자적인 no-replace rename으로 공개합니다. 추출 결과 `profile/`, `manifest.json`, `verified.json`을 보존합니다. 검증 중 실패한 staging은 비공개 상태로 보존하며 완료 receipt를 만들지 않습니다. publish/sync 실패 시에도 복원 파일·manifest·원본 archive/key는 유지합니다. 예상 공간은 암호문 크기로 제한된 복원 파일 한 벌과 기존 256 MiB 여유 조건을 사용합니다. 외부 복구 실행의 더 큰 디스크 여유 조건은 별도로 유지합니다. 형식·namespace 결합·64 GiB 데이터 및 8 MiB manifest 한도는 변경하지 않습니다. 키·원본·사본·실패 후보를 자동 삭제하지 않습니다.

사전 disk 여유 검사는 백업 예상 암호문, 추출 파일 한 벌과 256MiB 여유를 요구합니다. 동시 disk 사용·filesystem overhead까지 예약하지는 않습니다. 중간 쓰기·동기화 실패는 성공으로 표시하지 않습니다. `HOST_WRITE_UNCERTAIN`에서는 pending/target의 실제 상태를 확인하고 기존 파일을 보존한 채 새 이름으로 재시도합니다. Mac/Linux atomic no-replace primitive를 사용하며 실제 Linux·Windows·전원 차단 검증은 별도입니다.

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

현재 복구 자료와 서비스/runtime을 같은 잠금 범위에서 검사하는 개발 경로는 인증된 `manager-image-inventory.json`과 원본 bundle/백업 manifest에서 정확한 image export 크기를 얻습니다. 한 관측의 총 2 GiB 한도, 2 GiB 추가 여유, 6 GiB 디스크 floor를 유지하고, 함께 남아 있는 세 관측은 정확한 총량의 세 배를 예약합니다. 최댓값에서는 이전 10/14 GiB 요구량과 같습니다. 잘못된·비인증 크기, overflow, quota 초과는 거부하며 실제 export writer도 이미지별 정확한 byte/hash 한도를 계속 적용합니다. tmpfs DB 검사는 기존 메모리 한도를 유지합니다. 새 사본을 생성하는 persistent DB 경로의 16 GiB 조건은 바꾸지 않습니다.


## 호스트 암호문을 재사용하는 신뢰 기록 갱신

`refresh-checkpoint-trust`는 기존 전체 host archive를 재사용하면서 작은 최신 trust archive와 `pair-binding.bin`만 새 목적지에 만듭니다. 기존 catalog와 두 암호문의 인증된 identity, 독립적으로 보존된 현재 Store의 전체 이력과 과거 head를 검사합니다. 과거 head부터 현재까지 동일한 plan의 Prepared 상태만 허용합니다. Applying 또는 복구 상태를 이 명령으로 갱신할 수 없습니다.

```sh
./target/release/exhibitos-update refresh-checkpoint-trust \
  --profile /absolute/profile --installation default \
  --key-file /absolute/private/key.bin \
  --host-archive /absolute/previous/host.bin \
  --trust-archive /absolute/previous/trust.bin \
  --pair-binding /absolute/previous/pair-binding.bin \
  --destination /absolute/private/new-current-point \
  --host-writers-stopped --apps-closed
```

호스트 anchor·session과 Store fence를 유지한 채 전체 암호문을 인증하고 모든 현재 host 파일의 내용·권한·inventory를 세 번 재검사합니다. 최신 trust archive 생성과 새 catalog 작성 사이에도 key·이력·입력 암호문을 재검사합니다. whole plaintext, host archive 복사, external volume 사본은 만들지 않습니다. 기존 host archive는 새 catalog의 필수 외부 입력이므로 계속 보존해야 합니다. 목적지는 기존 파일을 덮어쓰지 않으며 실패 후보는 자동 삭제하지 않습니다.

이 receipt는 현재 암호문의 identity만 증명합니다. 과거 source/candidate 관측은 상속하지 않으므로 `sourcePlanBound`는 false입니다. 실제 관측으로 새 binding을 만들고 runtime, preflight, apply, rollback 검증을 별도로 수행해야 합니다. 이 명령은 만료된 release를 갱신하거나 실행을 허가하지 않습니다. Unix 개발 경로이며 Windows의 실제 파일 내구성·권한·GUI 검증을 대신하지 않습니다.


## 서버 검사와 같은 잠금 구간의 전체 비활성 복원

`qualify-full-recovery-runtime`은 `qualify-current-recovery-runtime`과 같은 인수를 사용하고 마지막 `--external-writers-quiesced --apps-closed` 앞에 `--host-extraction /absolute/private/new-extraction`을 추가합니다. 현재 세대에 실제 source/candidate 관측으로 결합한 checkpoint만 허용하며, 서명·artifact·시간 재검사와 Store/profile/source/candidate 작업 잠금을 유지합니다.

추출할 호스트 암호문 크기를 기존 인증된 image-export 성장·2GiB headroom·6GiB floor에 추가합니다. 새 목적지의 `host/`에는 전체 파일·manifest·권한을 실제로 복원하고 `trust/`에는 독립적으로 남아 있는 현재 authority와 바이트가 일치하는 전체 신뢰 기록을 추출합니다. 서버·runtime 관측 전후 및 최종 경계에서 모든 추출 파일의 hash·길이·권한·inventory와 최신 trust를 재검사합니다. 평문 중간 전체 사본은 만들지 않으며, 새 성공 추출물은 별도 영수증과 검사 후에만 정리할 수 있습니다. 실패 후보와 원본은 자동 삭제하지 않습니다.

`hostExtractionVerified`는 실제로 이 경로에서 추출했을 때만 true입니다. `inactiveFullRecovery`의 hostActivated와 liveAuthorityRestored는 false이며 이 검사는 원래 host 활성화, 잃어버린 최신 authority 복구, 서비스 데이터 활성화, owned preflight/apply 또는 rollback을 대신하지 않습니다. 기존 읽기 전용 명령은 추가 전체 추출 없이 이전 동작을 유지합니다.


## 스키마 변경 Runtime과 전체 비활성 복원 관측

`qualify-migrated-full-recovery-runtime`은 위 전체 복원 명령의 인수에
`--target-catalog /absolute/private/catalog.json`과
`--target-catalog-sha256 <64 lowercase hex>`를 마지막 두 동의 플래그 앞에 추가합니다.
현재 서명된 artifact의 실제 OCI layers·SQL catalog·source/target schema를 결합한
스키마 변경만 관측하며, catalog 파일과 부모 guard를 전체 callback 동안 유지하고 재검사합니다.
실제 scratch Runtime 관측과 전체 host/trust 비활성 추출을 동일한 잠금 구간에서 수행합니다.

이 명령도 admission, application, 선택 공간 활성화 또는 잃어버린 authority 복원을
허가하지 않습니다. 기존 owned update 경로의 동일 schema 조건은 유지합니다.
검사 결과는 해당 source commit과 실제 실행에 한해서 기록하며, 전체 실패 복원·cold/crash
검사가 완료되기 전에는 changed-schema 업데이트를 완료로 처리하지 않습니다.


## 독립 신뢰 보관소의 비활성 복원 검사

`exhibitos-update qualify-inactive-authority-recovery --profile /absolute/private/profile --installation default --destination /absolute/private/new-authority --apps-closed`는 현재 등록된 독립 보관소에서 전체 신뢰 기록을 새 공간으로 복원합니다. 프로필·호스트 잠금을 유지하며 원래 authority와 전체 기록이 일치하는지 재검사합니다. 목적지는 기존 파일을 덮어쓰지 않으며 profile/vault 안에 만들 수 없습니다. 결과는 실제 비활성 복원 관측만 증명하고 live authority/host/service 활성화와 preflight/update를 허가하지 않습니다. 기존 원래 authority가 없어졌을 때의 `restore-missing-authority` 검사는 별도로 수행해야 합니다.
