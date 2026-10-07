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
