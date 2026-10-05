#!/bin/bash
# Focused future-abort reproduction against the unchanged 0ee7cf86 executor.
# The test file is overlaid. The executor in /src is the archive.
set -u
echo "oo_task=sp06-cancel-future-raii-red"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a210"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "phase=red"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=4
export OO_A189_LIVE=1
mkdir -p /evidence/linux/red/phases /evidence/live
rustc -Vv | tee /evidence/linux/red/rustc-version.txt
got=$(rustc --version)
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; exit 2 ;;
esac
cd /src
if grep -n 'unsafe' crates/oo-gateway/src/query_executor.rs > /evidence/linux/red/unsafe-lines.txt; then
  echo "query_executor.rs contains unsafe" >&2
  exit 2
fi
: > /evidence/linux/red/unsafe-lines.txt
printf '%s\n' 0 > /evidence/linux/red/phases/unsafe.exit
sha256sum crates/oo-gateway/src/query_executor.rs > /evidence/linux/red/executor-sha256.txt
if grep -q 'ReservationGuard' crates/oo-gateway/src/query_executor.rs; then
  echo "red archive executor already contains the guard" >&2
  exit 2
fi

record() {
  printf '%s\n' "$2" > "/evidence/linux/red/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
}

so=$(find /cargo-home/clang-root/usr/lib -name 'libclang.so' -print -quit || true)
if [ -z "$so" ]; then
  echo "libclang missing from the smoke volume" >&2
  record libclang 2
  exit 2
fi
printf '%s\n' reused > /evidence/linux/red/libclang-reused.txt
record libclang 0
libdir=$(dirname "$so")
multiarch=/cargo-home/clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/clang-root/usr/share/pkgconfig"
printf '%s\n' "$LIBCLANG_PATH" > /evidence/linux/red/libclang-path.txt

echo "=== future abort against unchanged executor ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test query_executor_pg_live -- --nocapture live_permission_bound_select \
  > /evidence/linux/red/live.log 2>&1
live_exit=$?
set -e
record live "$live_exit"
if [ -f /evidence/live/oracle.txt ]; then
  cp /evidence/live/oracle.txt /evidence/linux/red/oracle.txt
fi
if [ "$live_exit" -eq 0 ]; then
  echo "red test passed; abort did not leave the session busy" >&2
  exit 1
fi
if ! grep -q 'owning cancel abort left' /evidence/linux/red/live.log; then
  echo "live failed before the abort assertion" >&2
  exit "$live_exit"
fi
echo "red_reproduced exit=${live_exit}"
exit "$live_exit"
