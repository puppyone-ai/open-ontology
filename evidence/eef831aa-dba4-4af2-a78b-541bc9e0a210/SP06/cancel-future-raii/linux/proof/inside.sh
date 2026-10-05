#!/bin/bash
# Green proof: future-local reservation guard on Linux arm64 Rust 1.94.
set -u
echo "oo_task=sp06-cancel-future-raii-proof"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a210"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "phase=proof"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=4
export OO_A189_LIVE=1
mkdir -p /evidence/linux/proof/phases /evidence/live
rustc -Vv | tee /evidence/linux/proof/rustc-version.txt
cargo -V | tee /evidence/linux/proof/cargo-version.txt
got=$(rustc --version)
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; exit 2 ;;
esac
cd /src
if grep -n 'unsafe' crates/oo-gateway/src/query_executor.rs > /evidence/linux/proof/unsafe-lines.txt; then
  echo "query_executor.rs contains unsafe" >&2
  exit 2
fi
: > /evidence/linux/proof/unsafe-lines.txt
printf '%s\n' 0 > /evidence/linux/proof/phases/unsafe.exit
if ! grep -q 'struct ReservationGuard' crates/oo-gateway/src/query_executor.rs; then
  echo "overlay executor is missing ReservationGuard" >&2
  exit 2
fi
sha256sum \
  crates/oo-gateway/src/query_executor.rs \
  crates/oo-gateway/tests/query_executor_pg_live.rs \
  contracts/gateway/query-executor-v1.md \
  > /evidence/linux/proof/owned-sha256.txt

record() {
  printf '%s\n' "$2" > "/evidence/linux/proof/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    exit "$2"
  fi
}

so=$(find /cargo-home/clang-root/usr/lib -name 'libclang.so' -print -quit || true)
if [ -z "$so" ]; then
  echo "libclang missing from the smoke volume" >&2
  record libclang 2
fi
printf '%s\n' reused > /evidence/linux/proof/libclang-reused.txt
record libclang 0
libdir=$(dirname "$so")
multiarch=/cargo-home/clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/clang-root/usr/share/pkgconfig"
printf '%s\n' "$LIBCLANG_PATH" > /evidence/linux/proof/libclang-path.txt

echo "=== cargo clean -p oo-gateway ==="
# A file mounted over the archive can keep the mtime cargo recorded for the
# unfixed executor. Drop this package so rustc opens the mounted source.
set +e
cargo clean -p oo-gateway --manifest-path Cargo.toml \
  > /evidence/linux/proof/clean.log 2>&1
clean_exit=$?
set -e
record clean "$clean_exit"

echo "=== cargo test -p oo-gateway --lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib \
  > /evidence/linux/proof/lib-test.log 2>&1
lib_exit=$?
set -e
record lib-test "$lib_exit"
if ! grep -q 'reservation_guard_drop_before_transport_releases_only_its_generation' /evidence/linux/proof/lib-test.log; then
  echo "lib tests did not include the reservation guard" >&2
  record lib-guard 2
fi
record lib-guard 0

echo "=== cargo test query_executor_pg_live ==="
rm -f /evidence/live/oracle.txt
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test query_executor_pg_live -- --nocapture live_permission_bound_select \
  > /evidence/linux/proof/live.log 2>&1
live_exit=$?
set -e
record live "$live_exit"
if [ -f /evidence/live/oracle.txt ]; then
  cp /evidence/live/oracle.txt /evidence/linux/proof/oracle.txt
fi

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
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib --test query_executor_pg_live -- -D warnings \
  > /evidence/linux/proof/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"
echo "proof_phases_ok"
