# Linux 코어 검증

이 절차는 Linux 커널에서 Rust lifecycle 코어의 실제 파일·잠금·권한·종료 검사를 실행한다. Manager GUI, 엔진 설치/운영과 전체 업데이트 인수 검증은 별도다. 공식 Rust1.99 ARM64 이미지를 digest로 고정한다.

2026-10-07 소스3126c31: Linux7.0.12-linuxkit/aarch64, Rust/Cargo1.99.0, 코어341PASS/0FAIL/32ignored7.17s. Update/rollback 선택 저장 세 경계의 SIGKILL9도 각각 통과했다. 총 실행51.95s에는 네트워크 dependency 준비·컴파일이 포함된다.

검사 전 호스트의 예상 추가 사용량1.2GB와6GiB 여유 기준을 확인한다. 이 계산은 실제 이미지 다운로드·빌드의 예약 보장이 아니며 환경에 맞게 넉넉히 산정한다. Source는 읽기 전용, registry/build는 크기가 제한된 tmpfs로 둔다. 검사 자료 자체는 일반 Linux writable layer에 둔다. 작은128MiB tmpfs 검사 공간은 제품의 원래6GiB 용량 조건을 충족하지 못해45개가 HOST_STORAGE_INSUFFICIENT 등으로 실패했다. 제품 조건을 낮추거나 테스트를 생략해 통과시키지 않는다.

다음 예는 저장소 루트에서 실행하며 새 비공개 결과 폴더와 고유 container 이름을 사용한다. 기존 앱·데이터·Docker socket을 연결하지 않는다. 종료 후 상태와 로그, 최종 바이너리를 확인하고 활성 프로세스가 없을 때만 재생성 자료를 정리한다. 실패 기록은 보존한다.

```sh
check_dir=$(mktemp -d)
chmod 700 "$check_dir"
check_name="exhibitos-linux-core-$(date +%s)-$$"
docker run --name "$check_name" --platform linux/arm64 \
  --memory 2g --memory-swap 2g --cpus 2 \
  --tmpfs /tmp:rw,size=64m \
  --tmpfs /build:rw,exec,size=512m \
  --tmpfs /cargo:rw,exec,size=256m \
  --mount "type=bind,src=$PWD,dst=/work,readonly" \
  --mount "type=bind,src=$check_dir,dst=/evidence" \
  -w /work \
  rust@sha256:59a1dc64fb43c0111c4acc48fda26a19806d0425d65786cbd59192ba9f99a790 \
  sh /work/scripts/check-linux-core-container.sh > "$check_dir/output.log" 2>&1
```

작업 종료 시 tmpfs 캐시는 사라지며 소스는 수정되지 않는다. Container writable layer의 작은 합성 자료는 별도다. Container를 자동 삭제하는 명령은 위 예제에 포함하지 않는다. 새 작업 전 실제 디스크·메모리 조건을 확인하고, stopped container 정리는 프로젝트의 복원점/기록 정책을 따른다.


## 실제 이미지 매핑 검사

코어 검사는 실제 Docker API 검사를 자동으로 실행하지 않는다. 다음 항목은 명시적으로 준비된 합성 fixture 이미지가 이미 로컬 엔진에 있을 때만 실행한다. `EXHIBITOS_NATIVE_TEST_IMAGE`에는 tag가 아닌 `sha256:` 형식의 정확한 이미지 ID를 넣는다. 검사 도우미가 기존 로컬 이미지의 실제 ID를 먼저 확인하며 이미지를 다운로드하지 않는다. 이 검사에서 사용하는 원본 합성 Runtime ID는 `source_images.rs`에 명시돼 있다. 다른 운영 이미지나 사용자 데이터로 대체하지 않는다.

```sh
EXHIBITOS_NATIVE_TEST_IMAGE="sha256:<qualified-local-helper-image-id>" \
  cargo test --release -p exhibitos-lifecycle --lib --locked \
  actual_engine_current_image_mappings -- --ignored --nocapture
```

Linux 컨테이너에서 실행하려면 Linux용 Docker CLI와 해당 로컬 개발 엔진의 socket이 필요하다. 위 코어 검사 예제에 socket을 무조건 추가하지 않는다. Socket을 연결한 검사는 실제 엔진 접근 권한을 갖는다. 이미지 매핑 항목 자체는 image inspect만 사용하며 container/volume 생성·삭제나 앱 시작을 하지 않는다.

2026-10-07 소스3a70cbd의 Linux ARM64 실제 이미지 매핑 검사1PASS/0FAIL(0.65s), 전체 준비·컴파일·실행44.32s. 읽기 전용 runner와 메모리 registry/build 캐시를 사용했고, 실행 전후 새 container/volume0을 확인했다. 최종 검사 실행 파일과 명령·로그·소스 SHA만 남겼다. 전체 migration/activation이나 Linux GUI 검증으로 확대하지 않는다.

다른 실제 엔진 fixture의 writer-census는 사용하지 않는 PostgreSQL 이미지의 기본 data 경로를1MiB tmpfs로 덮어 익명 볼륨 누적을 막는다. 외부 writer 여부·볼륨 소유권·설정 내용/권한 검사는 실제 engine과 새로운 합성 namespace를 사용하는 별도 검사다. 종료 후 원본과 미확인 볼륨은 보존하고, 정확한 fixture 생성 근거와 복원 증거가 있는 자료만 정리한다.
