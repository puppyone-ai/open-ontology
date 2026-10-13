#!/bin/bash
# Locked offline tests and Clippy for the product, then the two linux binaries.
set -u
export HOME=/tmp
export TMPDIR=/target/tmp
export CARGO_HOME=/cargo
export RUSTUP_HOME=/rustup
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
export CARGO_TARGET_DIR=/target/loop
mkdir -p /target/tmp /target/loop /evidence/phases
if [ -f /cargo/a405-clang-root/usr/lib/aarch64-linux-gnu/libclang.so ]; then
  export LIBCLANG_PATH=/cargo/a405-clang-root/usr/lib/aarch64-linux-gnu
fi
printf '%s\n' 'offline=yes cargo_update=no parent_acceptance=false' > /evidence/scope.txt
rustc --version > /evidence/rustc.txt 2>&1
cargo clippy --version > /evidence/clippy.txt 2>&1
fail=0
phase() {
  name="$1"
  shift
  "$@" > "/evidence/${name}.log" 2>&1
  code=$?
  printf '%s\n' "$code" > "/evidence/phases/${name}.exit"
  if [ "$code" -ne 0 ]; then
    fail=1
  fi
  return 0
}
phase product-test cargo test --manifest-path /src/tools/minimal-product-loop/Cargo.toml --locked --offline --jobs 1 -- --test-threads=1
phase product-clippy cargo clippy --manifest-path /src/tools/minimal-product-loop/Cargo.toml --locked --offline --jobs 1 --all-targets --no-deps -- -D warnings
phase logistics-test cargo test --manifest-path /src/testing/logistics-product-loop/Cargo.toml --locked --offline --jobs 1 -- --test-threads=1
phase logistics-clippy cargo clippy --manifest-path /src/testing/logistics-product-loop/Cargo.toml --locked --offline --jobs 1 --all-targets --no-deps -- -D warnings
phase product-build cargo build --manifest-path /src/tools/minimal-product-loop/Cargo.toml --locked --offline --jobs 1 --bin minimal-product-loop
phase mirror-build cargo build --manifest-path /src/tools/local-pg-mirror/Cargo.toml --locked --offline --jobs 1 --bin local-pg-mirror
phase splice rustc --edition 2021 --out-dir /target/loop /src/deploy/logistics-demo/linux/closure/splice.rs
if [ ! -x /target/loop/debug/minimal-product-loop ] || [ ! -x /target/loop/debug/local-pg-mirror ] || [ ! -x /target/loop/splice ]; then
  fail=1
  printf '%s\n' 'binary absent' >> /evidence/product-build.log
fi
printf '%s\n' "$fail" > /evidence/proof.exit
exit "$fail"
