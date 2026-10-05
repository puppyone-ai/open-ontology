#!/bin/bash
# Focused oo-gateway lib tests, scoped clippy, and the a180 live executor test.
# Rust 1.94, offline, locked. The pre-edit smoke logs stay in linux/phases.
set -u
echo "oo_task=sp06-query-executor-live"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a180"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "phase=live"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
rustc -Vv
cargo -V
got=$(rustc --version)
printf '%s\n' "$got"
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; exit 2 ;;
esac
cd /src
if [ ! -f crates/oo-gateway/src/query_executor.rs ]; then
  echo "query_executor.rs missing" >&2
  exit 2
fi
if [ ! -f crates/oo-gateway/tests/query_executor_pg_live.rs ]; then
  echo "live test missing" >&2
  exit 2
fi
if [ ! -f contracts/gateway/query-executor-v1.md ]; then
  echo "contract missing" >&2
  exit 2
fi
mkdir -p /evidence/linux/live
rawdir=$(cat /evidence/linux/libclang-dir.txt)
if [ "${rawdir#/tmp/}" != "$rawdir" ]; then
  libdir="/cargo-home/a180-clang-root${rawdir#/tmp/clang-root}"
else
  libdir="$rawdir"
fi
multiarch=/cargo-home/a180-clang-root/usr/lib/aarch64-linux-gnu
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I/cargo-home/a180-clang-root/usr/lib/llvm-19/lib/clang/19/include -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/a180-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a180-clang-root/usr/share/pkgconfig"
printf '%s\n' "$LIBCLANG_PATH" > /evidence/linux/live/libclang-path.txt
so=$(ls "$LIBCLANG_PATH"/libclang.so* 2>/dev/null | head -n 1 || true)
if [ -n "$so" ]; then
  ldd "$so" > /evidence/linux/live/libclang-ldd.txt
fi

record() {
  printf '%s\n' "$2" > "/evidence/linux/live/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    exit "$2"
  fi
}

set +e
cargo fetch --manifest-path Cargo.toml --locked \
  > /evidence/linux/live/fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"

set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib \
  > /evidence/linux/live/lib-test.log 2>&1
lib_exit=$?
set -e
record lib-test "$lib_exit"

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/live/clippy-component.log 2>&1
  clippy_add=$?
  set -e
  record clippy-component "$clippy_add"
else
  record clippy-component 0
fi

set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib --test query_executor_pg_live -- -D warnings \
  > /evidence/linux/live/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

set +e
OO_A180_LIVE=1 cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test query_executor_pg_live -- --nocapture \
  > /evidence/linux/live/pg-test.log 2>&1
pg_exit=$?
set -e
record pg-test "$pg_exit"
echo "live_phases_ok"
