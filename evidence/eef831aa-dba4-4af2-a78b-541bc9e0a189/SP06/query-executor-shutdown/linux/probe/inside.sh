#!/bin/bash
# Baseline reproduction. The archive is 8a289f0c plus this one overlay test.
set -u
echo "oo_task=sp06-query-executor-shutdown-repro"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a189"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "phase=baseline-repro"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=4
mkdir -p /evidence/linux/probe/phases
rustc --version | tee /evidence/linux/probe/rustc-version.txt
got=$(rustc --version)
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; exit 2 ;;
esac
cd /src
if [ ! -f crates/oo-gateway/src/query_executor.rs ]; then
  echo "query_executor.rs missing" >&2
  exit 2
fi
if [ -f crates/oo-gateway/src/parser_worker.rs ]; then
  echo "archive contains parser_worker.rs" >&2
  exit 2
fi
if [ ! -f crates/oo-gateway/tests/query_executor_repro.rs ]; then
  echo "repro overlay missing" >&2
  exit 2
fi
expected=ed3e1c4331d58fe79c449cdb06c6c39a40992bf20e3273d244566761c0605a1b
got_hash=$(sha256sum crates/oo-gateway/src/query_executor.rs | awk '{print $1}')
printf 'query_executor=%s\n' "$got_hash" > /evidence/linux/probe/executor-sha256.txt
if [ "$got_hash" != "$expected" ]; then
  echo "query_executor.rs is not the baseline blob" >&2
  exit 2
fi
if grep -n 'unsafe' crates/oo-gateway/src/query_executor.rs > /evidence/linux/probe/unsafe-lines.txt; then
  echo "query_executor.rs contains unsafe" >&2
  exit 2
fi
printf '%s\n' 0 > /evidence/linux/probe/phases/unsafe.exit

so=$(find /cargo-home/clang-root/usr/lib -name 'libclang.so' -print -quit || true)
if [ -z "$so" ]; then
  echo "libclang.so missing from the smoke cargo volume" >&2
  printf '%s\n' 1 > /evidence/linux/probe/phases/libclang.exit
  exit 1
fi
printf '%s\n' reused > /evidence/linux/probe/libclang-reused.txt
libdir=$(dirname "$so")
multiarch=/cargo-home/clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/clang-root/usr/bin:${PATH}"
printf '%s\n' "$LIBCLANG_PATH" > /evidence/linux/probe/libclang-path.txt
printf '%s\n' 0 > /evidence/linux/probe/phases/libclang.exit

echo "=== cargo test baseline repro ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test query_executor_repro -- --nocapture \
  > /evidence/linux/probe/repro.log 2>&1
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/linux/probe/phases/repro.exit
echo "phase_exit=$test_exit phase=repro"
exit "$test_exit"
