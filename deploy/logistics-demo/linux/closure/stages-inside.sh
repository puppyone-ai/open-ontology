#!/bin/bash
# Build the stage CLIs from their own manifests. Does not rebuild query-pg or the product.
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
printf '%s\n' 'offline=yes cargo_update=no parent_acceptance=false scope=stage-clis' > /evidence/scope.txt
rustc --version > /evidence/rustc.txt 2>&1
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
phase rules-build cargo build --manifest-path /src/tools/selfbuild-rules/Cargo.toml --locked --offline --jobs 1 --bin selfbuild-rules
phase suggestions-build cargo build --manifest-path /src/tools/selfbuild-suggestions/Cargo.toml --locked --offline --jobs 1 --bin selfbuild-suggestions
phase cards-build cargo build --manifest-path /src/tools/pg-builder/Cargo.toml --locked --offline --jobs 1 --bin pg-builder
sha256sum /target/loop/debug/selfbuild-rules /target/loop/debug/selfbuild-suggestions /target/loop/debug/pg-builder > /evidence/binaries.sha256 2>/dev/null || true
printf '%s\n' "$fail" > /evidence/proof.exit
exit "$fail"
