#!/bin/bash
# Focused oo-gateway catalog build on Linux arm64 Rust 1.94.
# The mounted tree is the fixed archive plus this task's overlay.
# No cargo update. The lock stays --locked.
set -u
echo "oo_task=sp06-policy-catalog-focused"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a190"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "phase=focused"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
cd /src
mkdir -p /evidence/focused/phases
rustc -Vv
cargo -V
got=$(rustc --version)
printf '%s\n' "$got"
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; exit 2 ;;
esac
if [ ! -f crates/oo-gateway/src/policy_catalog.rs ]; then
  echo "overlay is missing policy_catalog.rs" >&2
  exit 2
fi
if [ ! -f crates/oo-gateway/src/sql_guard.rs ] || [ ! -f crates/oo-gateway/src/query_executor.rs ]; then
  echo "ancestor gateway sources missing" >&2
  exit 2
fi

record() {
  printf '%s\n' "$2" > "/evidence/focused/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    exit "$2"
  fi
}

libdir=/cargo-home/a190-clang-root/usr/lib/llvm-19/lib
multiarch=/cargo-home/a190-clang-root/usr/lib/aarch64-linux-gnu
if [ ! -e "$libdir/libclang.so" ] && [ ! -e "$multiarch/libclang-19.so.19" ]; then
  echo "libclang from the preedit smoke is missing" >&2
  record libclang 2
fi
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I/cargo-home/a190-clang-root/usr/lib/llvm-19/lib/clang/19/include -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/a190-clang-root/usr/bin:${PATH}"
record libclang 0

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^rustfmt'; then
  set +e
  rustup component add rustfmt --toolchain 1.94.0 > /evidence/focused/rustfmt-component.log 2>&1
  fmt_add=$?
  set -e
  record rustfmt-component "$fmt_add"
else
  record rustfmt-component 0
fi
if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/focused/clippy-component.log 2>&1
  clippy_add=$?
  set -e
  record clippy-component "$clippy_add"
else
  record clippy-component 0
fi

set +e
rustfmt --edition 2024 \
  crates/oo-gateway/src/policy_catalog.rs \
  crates/oo-gateway/tests/policy_catalog_pg_live.rs \
  > /evidence/focused/rustfmt.log 2>&1
fmt_exit=$?
set -e
record rustfmt "$fmt_exit"

echo "=== cargo test -p oo-gateway --lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib \
  > /evidence/focused/lib-test.log 2>&1
test_exit=$?
set -e
record lib-test "$test_exit"
tail -30 /evidence/focused/lib-test.log

echo "=== cargo test -p oo-gateway --test policy_catalog_pg_live ==="
set +e
env -u OO_A190_LIVE cargo test --manifest-path Cargo.toml --locked --offline \
  -p oo-gateway --test policy_catalog_pg_live -- --nocapture \
  > /evidence/focused/compile-test.log 2>&1
compile_exit=$?
set -e
record compile-test "$compile_exit"
tail -20 /evidence/focused/compile-test.log

echo "=== cargo clippy ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway \
  --lib --test policy_catalog_pg_live -- -D warnings \
  > /evidence/focused/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"
tail -40 /evidence/focused/clippy.log

echo "=== live policy catalog ==="
set +e
OO_A190_LIVE=1 cargo test --manifest-path Cargo.toml --locked --offline \
  -p oo-gateway --test policy_catalog_pg_live -- --nocapture \
  > /evidence/focused/live-test.log 2>&1
live_exit=$?
set -e
record live-test "$live_exit"
tail -60 /evidence/focused/live-test.log
exit "$live_exit"
