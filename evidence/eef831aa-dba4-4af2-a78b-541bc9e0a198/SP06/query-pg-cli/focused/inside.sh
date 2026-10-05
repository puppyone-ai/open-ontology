#!/bin/bash
# Focused query-pg proof on the frozen archive plus the owned overlay.
# cargo check --offline may add the oo-gateway serde edge. Any other lock
# change stops the run.
set -u
echo "oo_task=sp06-query-pg-cli-focused"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a198"
echo "phase=focused"
uname -a
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
if [ ! -f crates/oo-gateway/src/bin/query-pg.rs ]; then
  echo "overlay is missing query-pg.rs" >&2
  exit 2
fi
if [ ! -f crates/oo-gateway/tests/query_pg_cli.rs ]; then
  echo "overlay is missing query_pg_cli.rs" >&2
  exit 2
fi
if [ ! -f contracts/gateway/query-pg-cli-v1.md ]; then
  echo "overlay is missing the contract" >&2
  exit 2
fi
mkdir -p /evidence/focused/phases
record() {
  printf '%s\n' "$2" > "/evidence/focused/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    exit "$2"
  fi
}

rawdir=$(cat /evidence/linux/libclang-dir.txt)
case "$rawdir" in
  /cargo-home/*) libdir="$rawdir" ;;
  /tmp/clang-root/*)
    libdir="/cargo-home/a198-clang-root${rawdir#/tmp/clang-root}"
    ;;
  *) echo "unexpected libclang dir" >&2; exit 2 ;;
esac
multiarch=/cargo-home/a198-clang-root/usr/lib/aarch64-linux-gnu
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I/cargo-home/a198-clang-root/usr/lib/llvm-19/lib/clang/19/include -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/a198-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a198-clang-root/usr/share/pkgconfig"
if ! /lib/ld-linux-aarch64.so.1 --list "${multiarch}/libclang-19.so.19" > /evidence/focused/libclang-ldd.txt 2>&1; then
  record libclang-load 1
fi
if grep -q 'not found' /evidence/focused/libclang-ldd.txt; then
  record libclang-load 1
fi
record libclang-load 0

if [ ! -f /evidence/focused/Cargo.lock.baseline ]; then
  cp Cargo.lock /evidence/focused/Cargo.lock.baseline
fi
cp /evidence/focused/Cargo.lock.baseline Cargo.lock

echo "=== cargo check --offline query-pg ==="
set +e
cargo check --manifest-path Cargo.toml --offline -p oo-gateway --bin query-pg --tests \
  > /evidence/focused/check.log 2>&1
check_exit=$?
set -e
printf '%s\n' "$check_exit" > /evidence/focused/phases/check.exit
echo "phase_exit=$check_exit phase=check"
tail -50 /evidence/focused/check.log || true
cp Cargo.lock /evidence/focused/Cargo.lock.after-check

rustc --edition 2021 /evidence/focused/lock-edge.rs -o /cargo-home/a198-lock-edge
set +e
/cargo-home/a198-lock-edge /evidence/focused/Cargo.lock.baseline /evidence/focused/Cargo.lock.after-check \
  > /evidence/focused/lock-edge.txt 2> /evidence/focused/lock-edge.err
edge_exit=$?
set -e
printf '%s\n' "$edge_exit" > /evidence/focused/phases/lock-edge.exit
echo "phase_exit=$edge_exit phase=lock-edge"
cat /evidence/focused/lock-edge.txt /evidence/focused/lock-edge.err || true
if [ "$edge_exit" -ne 0 ]; then
  exit "$edge_exit"
fi
if [ "$check_exit" -ne 0 ]; then
  exit "$check_exit"
fi

echo "=== cargo test query_pg_cli offline ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test query_pg_cli \
  > /evidence/focused/test-offline.log 2>&1
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/focused/phases/test-offline.exit
echo "phase_exit=$test_exit phase=test-offline"
tail -60 /evidence/focused/test-offline.log || true
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi

echo "=== cargo test query_pg_cli live ==="
set +e
OO_A198_LIVE=1 cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test query_pg_cli live_query_pg_against_private_postgres -- --nocapture \
  > /evidence/focused/test-live.log 2>&1
live_exit=$?
set -e
printf '%s\n' "$live_exit" > /evidence/focused/phases/test-live.exit
echo "phase_exit=$live_exit phase=test-live"
tail -80 /evidence/focused/test-live.log || true
if [ "$live_exit" -ne 0 ]; then
  exit "$live_exit"
fi

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  echo "=== rustup component add clippy --toolchain 1.94.0 ==="
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/focused/clippy-component.log 2>&1
  clippy_add=$?
  set -e
  printf '%s\n' "$clippy_add" > /evidence/focused/phases/clippy-component.exit
  echo "phase_exit=$clippy_add phase=clippy-component"
  if [ "$clippy_add" -ne 0 ]; then
    exit "$clippy_add"
  fi
else
  echo "clippy_component=already_installed"
  printf '%s\n' 0 > /evidence/focused/phases/clippy-component.exit
fi

echo "=== cargo clippy query-pg ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --bin query-pg --test query_pg_cli -- -D warnings \
  > /evidence/focused/clippy.log 2>&1
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/focused/phases/clippy.exit
echo "phase_exit=$clippy_exit phase=clippy"
tail -40 /evidence/focused/clippy.log || true
if [ "$clippy_exit" -ne 0 ]; then
  exit "$clippy_exit"
fi

echo "=== cargo test oo-gateway lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib \
  > /evidence/focused/lib-test.log 2>&1
lib_exit=$?
set -e
printf '%s\n' "$lib_exit" > /evidence/focused/phases/lib-test.exit
echo "phase_exit=$lib_exit phase=lib-test"
tail -30 /evidence/focused/lib-test.log || true
exit "$lib_exit"
