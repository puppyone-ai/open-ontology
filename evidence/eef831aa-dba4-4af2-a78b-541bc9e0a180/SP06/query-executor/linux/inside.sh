#!/bin/bash
# Pre-edit oo-gateway lib smoke on Linux arm64 Rust 1.94.
# Refuses if the executor source already exists. No cargo update. Lock stays --locked.
set -u
echo "oo_task=sp06-query-executor-preedit"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a180"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "phase=preedit"
echo "fetch_argv=cargo fetch --manifest-path Cargo.toml --locked"
echo "test_argv=cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib"
echo "clippy_argv=cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib -- -D warnings"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
echo "RUSTUP_TOOLCHAIN=$RUSTUP_TOOLCHAIN"
echo "CARGO_HOME=$CARGO_HOME"
rustc -Vv
cargo -V
got=$(rustc --version)
printf '%s\n' "$got"
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; exit 2 ;;
esac
cd /src
if [ ! -f Cargo.lock ] || [ ! -f crates/oo-gateway/Cargo.toml ]; then
  echo "snapshot missing cargo inputs" >&2
  exit 2
fi
if [ ! -f crates/oo-gateway/src/sql_guard.rs ]; then
  echo "ancestor sql_guard.rs missing" >&2
  exit 2
fi
if [ -f crates/oo-gateway/src/query_executor.rs ]; then
  echo "preedit saw query_executor.rs; refusing" >&2
  exit 2
fi
mkdir -p /evidence/linux/phases

record() {
  printf '%s\n' "$2" > "/evidence/linux/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    exit "$2"
  fi
}

if [ ! -s /evidence/linux/libclang-dir.txt ]; then
  set +e
  bash /src/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a180/SP06/query-executor/linux/libclang-extract.sh \
    > /evidence/linux/libclang-extract.log 2>&1
  extract_exit=$?
  set -e
  record libclang "$extract_exit"
else
  record libclang 0
fi
rawdir=$(cat /evidence/linux/libclang-dir.txt)
if [ "${rawdir#/tmp/}" != "$rawdir" ]; then
  cp /evidence/linux/libclang-dir.txt /evidence/linux/libclang-dir-noexec.txt
  if [ ! -d /cargo-home/a180-clang-root/usr/lib ]; then
    cp -a /tmp/clang-root /cargo-home/a180-clang-root
  fi
  libdir="/cargo-home/a180-clang-root${rawdir#/tmp/clang-root}"
  printf '%s\n' "$libdir" > /evidence/linux/libclang-dir.txt
else
  libdir="$rawdir"
fi
multiarch=/cargo-home/a180-clang-root/usr/lib/aarch64-linux-gnu
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I/cargo-home/a180-clang-root/usr/lib/llvm-19/lib/clang/19/include -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/a180-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a180-clang-root/usr/share/pkgconfig"
{
  echo "LIBCLANG_PATH=$LIBCLANG_PATH"
  echo "LD_LIBRARY_PATH=$LD_LIBRARY_PATH"
  echo "BINDGEN_EXTRA_CLANG_ARGS=$BINDGEN_EXTRA_CLANG_ARGS"
  ls -l "$libdir"/libclang.so* || true
  pkg-config --modversion libclang || true
  /lib/ld-linux-aarch64.so.1 --list "${multiarch}/libclang-19.so.19" || true
} > /evidence/linux/libclang.txt
if ! /lib/ld-linux-aarch64.so.1 --list "${multiarch}/libclang-19.so.19" > /evidence/linux/libclang-ldd.txt 2>&1; then
  record libclang-load 1
fi
if grep -q 'not found' /evidence/linux/libclang-ldd.txt; then
  record libclang-load 1
fi
record libclang-load 0

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  echo "=== rustup component add clippy --toolchain 1.94.0 ==="
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/clippy-component.log 2>&1
  clippy_add=$?
  set -e
  record clippy-component "$clippy_add"
else
  echo "clippy_component=already_installed"
  record clippy-component 0
fi

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked > /evidence/linux/smoke-fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"
cp /evidence/linux/phases/fetch.exit /evidence/linux/phases/smoke-fetch.exit

echo "=== cargo test -p oo-gateway --lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib \
  > /evidence/linux/lib-test.log 2>&1
test_exit=$?
set -e
echo "phase_exit=$test_exit phase=cargo_test_oo_gateway_lib"
printf '%s\n' "$test_exit" > /evidence/linux/phases/test.exit
cp /evidence/linux/phases/test.exit /evidence/linux/phases/smoke-test.exit
tail -40 /evidence/linux/lib-test.log

echo "=== cargo clippy -p oo-gateway --lib ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib -- -D warnings \
  > /evidence/linux/clippy.log 2>&1
clippy_exit=$?
set -e
echo "phase_exit=$clippy_exit phase=cargo_clippy_oo_gateway_lib"
printf '%s\n' "$clippy_exit" > /evidence/linux/phases/clippy.exit
cp /evidence/linux/phases/clippy.exit /evidence/linux/phases/smoke-clippy.exit
tail -40 /evidence/linux/clippy.log
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi
exit "$clippy_exit"
