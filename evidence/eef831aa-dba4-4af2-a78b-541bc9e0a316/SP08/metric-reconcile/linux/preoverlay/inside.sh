#!/bin/bash
# Pre-edit Linux arm64 Rust 1.94 smoke of fixed archive 99ac19af.
# Focused oo-gateway sql_guard tests. No metric-reconcile overlay. No PostgreSQL.
# Libclang extract adapted from a200 smoke extract-libclang.sh.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
cd /src
mkdir -p /evidence/linux/preoverlay/phases /cargo-home/bin

record() {
  printf '%s\n' "$2" > "/evidence/linux/preoverlay/phases/$1.exit"
  echo "PHASE $1=$2"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/preoverlay/smoke.exit
    exit "$2"
  fi
}

echo "=== identity ==="
rustc -vV | tee /evidence/linux/preoverlay/rustc-verbose.txt
cargo -V | tee /evidence/linux/preoverlay/cargo-version.txt
uname -a | tee /evidence/linux/preoverlay/uname.txt
got=$(rustc --version)
printf '%s\n' "$got" > /evidence/linux/preoverlay/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) record toolchain 0 ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac

if [ -e tools/metric-reconcile ]; then
  echo "overlay present during preoverlay" >&2
  record overlay-absent 2
fi
printf '%s\n' overlay_absent > /evidence/linux/preoverlay/overlay-absent.txt
record overlay-absent 0

if [ ! -f crates/oo-gateway/src/sql_guard.rs ]; then
  echo "fixed archive is missing sql_guard.rs" >&2
  record source 2
fi
record source 0

rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
record sha256-tool $?
rustc --edition 2021 /tools/select-debs.rs -o /cargo-home/bin/select-debs
record select-debs-tool $?
export PATH="/cargo-home/bin:${PATH}"

/cargo-home/bin/sha256 file \
  crates/oo-gateway/src/sql_guard.rs \
  crates/oo-gateway/src/query_executor.rs \
  crates/oo-gateway/src/policy.rs \
  crates/oo-core/src/oos/model.rs \
  crates/oo-core/src/decimal.rs \
  Cargo.lock \
  > /evidence/linux/preoverlay/baseline-sha256.txt
record baseline-hash $?

if [ ! -s /evidence/linux/smoke/libclang-dir.txt ] || [ ! -d /cargo-home/a316-clang-root/usr/lib ]; then
  set +e
  bash /extract-libclang.sh > /evidence/linux/preoverlay/libclang-extract.log 2>&1
  extract_exit=$?
  set -e
  record libclang "$extract_exit"
else
  echo "libclang_root=reused" > /evidence/linux/preoverlay/libclang-extract.log
  record libclang 0
fi
libdir=$(cat /evidence/linux/smoke/libclang-dir.txt)
multiarch=/cargo-home/a316-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a316-clang-root/usr/lib/llvm-19/lib/clang/19/include
devso=$(find /cargo-home/a316-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  libdir=$(dirname "$devso")
fi
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a316-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a316-clang-root/usr/share/pkgconfig"
{
  echo "LIBCLANG_PATH=$LIBCLANG_PATH"
  test -f "$llvm_inc/stdbool.h" && echo "stdbool.h=present" || echo "stdbool.h=absent"
} > /evidence/linux/preoverlay/libclang.txt
if [ ! -f "$llvm_inc/stdbool.h" ] || [ -z "$libdir" ]; then
  record libclang-includes 1
fi
record libclang-includes 0
printf '%s\n' "no" > /evidence/linux/preoverlay/cargo-update-invoked.txt

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked > /evidence/linux/preoverlay/fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"

echo "=== cargo test sql_guard ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test sql_guard -- --test-threads=1 \
  > /evidence/linux/preoverlay/test.log 2>&1
test_exit=$?
set -e
record test "$test_exit"

/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}' > /evidence/linux/preoverlay/lock-after.sha256
before=$(awk '$2 ~ /Cargo.lock$/ {print $1}' /evidence/linux/preoverlay/baseline-sha256.txt)
after=$(cat /evidence/linux/preoverlay/lock-after.sha256)
if [ "$before" != "$after" ]; then
  echo "root lock changed during preoverlay" >&2
  record lock-hash 1
fi
record lock-hash 0
printf '%s\n' 0 > /evidence/linux/preoverlay/smoke.exit
echo "preoverlay_ok"
