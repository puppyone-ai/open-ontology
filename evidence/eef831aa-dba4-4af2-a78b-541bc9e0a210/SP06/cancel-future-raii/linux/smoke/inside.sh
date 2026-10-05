#!/bin/bash
# Pre-edit focused oo-gateway smoke on Linux arm64 Rust 1.94.
# The source tree is an immutable archive of 0ee7cf86. No cargo update.
set -u
echo "oo_task=sp06-cancel-future-raii-preedit"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a210"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "phase=preedit"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=4
mkdir -p /evidence/linux/smoke/phases
rustc -Vv | tee /evidence/linux/smoke/rustc-version.txt
cargo -V | tee /evidence/linux/smoke/cargo-version.txt
got=$(rustc --version)
printf '%s\n' "$got"
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; exit 2 ;;
esac
cd /src
if [ ! -f crates/oo-gateway/src/query_executor.rs ]; then
  echo "query_executor.rs missing from the immutable archive" >&2
  exit 2
fi
if grep -n 'unsafe' crates/oo-gateway/src/query_executor.rs > /evidence/linux/smoke/unsafe-lines.txt; then
  echo "query_executor.rs contains unsafe" >&2
  exit 2
fi
: > /evidence/linux/smoke/unsafe-lines.txt
printf '%s\n' 0 > /evidence/linux/smoke/phases/unsafe.exit
sha256sum \
  crates/oo-gateway/src/query_executor.rs \
  crates/oo-gateway/tests/query_executor_pg_live.rs \
  contracts/gateway/query-executor-v1.md \
  > /evidence/linux/smoke/owned-sha256.txt
printf '%s\n' "fixed=0ee7cf86ad443ee1d84fe9fe3ed3f7d8847799c1 overlay=absent" > /evidence/linux/smoke/overlay-absent.txt

record() {
  printf '%s\n' "$2" > "/evidence/linux/smoke/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    exit "$2"
  fi
}

so=$(find /cargo-home/clang-root/usr/lib -name 'libclang.so' -print -quit || true)
if [ -z "$so" ]; then
  set +e
  bash /extract-libclang.sh > /evidence/linux/smoke/libclang-extract.log 2>&1
  extract_exit=$?
  set -e
  record libclang "$extract_exit"
  so=$(find /cargo-home/clang-root/usr/lib -name 'libclang.so' -print -quit || true)
else
  printf '%s\n' reused > /evidence/linux/smoke/libclang-reused.txt
  record libclang 0
fi
if [ -z "$so" ]; then
  echo "libclang.so missing" >&2
  exit 2
fi
libdir=$(dirname "$so")
multiarch=/cargo-home/clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/clang-root/usr/share/pkgconfig"
printf '%s\n' "$LIBCLANG_PATH" > /evidence/linux/smoke/libclang-path.txt
/lib/ld-linux-aarch64.so.1 --list "$so" > /evidence/linux/smoke/libclang-ldd.txt
if grep -q 'not found' /evidence/linux/smoke/libclang-ldd.txt; then
  echo "libclang did not resolve" >&2
  record libclang-load 1
fi
if ! grep -q 'libLLVM' /evidence/linux/smoke/libclang-ldd.txt; then
  echo "libclang did not resolve LLVM" >&2
  record libclang-load 1
fi
record libclang-load 0

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked > /evidence/linux/smoke/fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"

echo "=== cargo test -p oo-gateway --lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib \
  > /evidence/linux/smoke/lib-test.log 2>&1
test_exit=$?
set -e
record lib-test "$test_exit"

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/smoke/clippy-component.log 2>&1
  clippy_add=$?
  set -e
  record clippy-component "$clippy_add"
else
  printf '%s\n' present > /evidence/linux/smoke/clippy-component.log
  record clippy-component 0
fi

echo "=== cargo clippy focused gateway ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib --test query_executor_pg_live -- -D warnings \
  > /evidence/linux/smoke/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"
echo "smoke_phases_ok"
