#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
# Run inside the documented isolated Linux container; source mount is read-only.
set -eu
export CARGO_HOME=/cargo
export CARGO_TARGET_DIR=/build
export RUSTUP_TOOLCHAIN=1.99.0
export CARGO_BUILD_JOBS=2
export TMPDIR=/qualification
mkdir -p /qualification
rustc --version
cargo --version
uname -a
cargo test --release -p exhibitos-lifecycle --lib --locked --manifest-path /work/Cargo.toml --no-run
mkdir -p /evidence/bin
for executable in /build/release/deps/exhibitos_lifecycle-*; do
  if [ -f "$executable" ] && [ -x "$executable" ]; then
    cp "$executable" /evidence/bin/
    sha256sum "$executable"
  fi
done
cargo test --release -p exhibitos-lifecycle --lib --locked --manifest-path /work/Cargo.toml -- --nocapture
