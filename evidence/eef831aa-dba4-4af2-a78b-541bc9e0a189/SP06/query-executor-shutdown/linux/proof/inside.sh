#!/bin/bash
# Post-edit proof. Immutable 8a289f0c archive plus the three owned overlays.
set -u
echo "oo_task=sp06-query-executor-shutdown-proof"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a189"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "phase=proof"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=4
export OO_A189_LIVE=1
mkdir -p /evidence/linux/proof/phases
rustc --version | tee /evidence/linux/proof/rustc-version.txt
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
if grep -n 'unsafe' crates/oo-gateway/src/query_executor.rs > /evidence/linux/proof/unsafe-lines.txt; then
  echo "query_executor.rs contains unsafe" >&2
  exit 2
fi
printf '%s\n' 0 > /evidence/linux/proof/phases/unsafe.exit
sha256sum \
  crates/oo-gateway/src/query_executor.rs \
  crates/oo-gateway/tests/query_executor_pg_live.rs \
  contracts/gateway/query-executor-v1.md \
  > /evidence/linux/proof/owned-sha256.txt
if ! grep -q 'oo-a189-pg' crates/oo-gateway/tests/query_executor_pg_live.rs; then
  echo "live test was not retargeted" >&2
  exit 2
fi

so=$(find /cargo-home/clang-root/usr/lib -name 'libclang.so' -print -quit || true)
if [ -z "$so" ]; then
  echo "libclang.so missing from the smoke cargo volume" >&2
  printf '%s\n' 1 > /evidence/linux/proof/phases/libclang.exit
  exit 1
fi
printf '%s\n' reused > /evidence/linux/proof/libclang-reused.txt
libdir=$(dirname "$so")
multiarch=/cargo-home/clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/clang-root/usr/bin:${PATH}"
printf '%s\n' "$LIBCLANG_PATH" > /evidence/linux/proof/libclang-path.txt
printf '%s\n' 0 > /evidence/linux/proof/phases/libclang.exit

record() {
  printf '%s\n' "$2" > "/evidence/linux/proof/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    exit "$2"
  fi
}

echo "=== cargo test -p oo-gateway --lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib \
  > /evidence/linux/proof/lib-test.log 2>&1
test_exit=$?
set -e
record lib-test "$test_exit"

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/proof/clippy-component.log 2>&1
  clippy_add=$?
  set -e
  record clippy-component "$clippy_add"
else
  printf '%s\n' present > /evidence/linux/proof/clippy-component.log
  record clippy-component 0
fi

echo "=== cargo clippy focused gateway ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline \
  -p oo-gateway --lib --test query_executor_pg_live -- -D warnings \
  > /evidence/linux/proof/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

echo "=== cargo test query_executor_pg_live ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline \
  -p oo-gateway --test query_executor_pg_live -- --nocapture \
  > /evidence/linux/proof/live.log 2>&1
live_exit=$?
set -e
record live "$live_exit"
echo "proof_result=compared"
exit 0
