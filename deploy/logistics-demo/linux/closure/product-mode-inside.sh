#!/bin/bash
# Focused product rebuild after the guest directory mode fix.
# Does not rebuild query-pg, local-pg-mirror, or the splice helper.
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
  export LD_LIBRARY_PATH=/cargo/a405-clang-root/usr/lib/aarch64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
  export BINDGEN_EXTRA_CLANG_ARGS="-I/cargo/a405-clang-root/usr/lib/llvm-19/lib/clang/19/include -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include"
fi
printf '%s\n' 'offline=yes cargo_update=no parent_acceptance=false scope=product-mode-only' > /evidence/scope.txt
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
phase logistics-build cargo build --manifest-path /src/testing/logistics-product-loop/Cargo.toml --locked --offline --jobs 1 --bin logistics-product-loop
phase product-build cargo build --manifest-path /src/tools/minimal-product-loop/Cargo.toml --locked --offline --jobs 1 --bin minimal-product-loop
if [ ! -x /target/loop/debug/minimal-product-loop ]; then
  fail=1
  printf '%s\n' 'product binary absent' >> /evidence/product-build.log
fi
sha256sum /target/loop/debug/minimal-product-loop > /evidence/binaries.sha256 2>/dev/null || true
printf '%s\n' "$fail" > /evidence/proof.exit
exit "$fail"
