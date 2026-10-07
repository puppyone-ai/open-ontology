#!/bin/bash
# Focused Linux arm64 Rust 1.94 smoke before the mapping-reader typmod edit.
# Fixed archive plus the six-file numeric overlay. mapping_reader_access.rs
# stays the archive copy, which checks the type name only.
# Origin: evidence/.../numeric-query/linux/live-3/inside.sh
# Changes: stop after the gateway lib tests and the lock hash. No live query.
set -u
echo "oo_task=sp06-numeric-query-smoke-map"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a327"
echo "oo_fixed=e6ebf93b5ac261c097a24d145611e0300e4f88b5"
echo "oo_platform=linux/arm64"
echo "phase=smoke-map"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/a327
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /evidence/linux/smoke-map/phases /cargo-home/bin
cd /src
unset OO_A327_LIVE

record() {
  mkdir -p /evidence/linux/smoke-map/phases
  printf '%s\n' "$2" > "/evidence/linux/smoke-map/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/smoke-map/smoke.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/smoke-map/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
record toolchain 0

if [ ! -x /cargo-home/bin/sha256 ]; then
  rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
fi
export PATH="/cargo-home/bin:${PATH}"

for path in \
  crates/oo-gateway/src/reviewed_policy.rs \
  crates/oo-gateway/src/query_executor.rs \
  crates/oo-gateway/src/bin/query-pg.rs \
  crates/oo-gateway/tests/a327_numeric.rs \
  crates/oo-gateway/src/policy.rs \
  crates/oo-gateway/src/policy_catalog.rs
do
  if [ ! -f "$path" ]; then
    echo "overlay missing $path" >&2
    record overlay 2
  fi
done
if grep -q 'grantable_base_type' crates/oo-gateway/src/mapping_reader_access.rs; then
  echo "smoke tree already bounds mapping-reader numeric" >&2
  record overlay 2
fi
if ! grep -q 'SUPPORTED_TYPES.contains' crates/oo-gateway/src/mapping_reader_access.rs; then
  echo "smoke tree lost the name-only mapping check" >&2
  record overlay 2
fi
if ! grep -q 'fn grantable_base_type' crates/oo-gateway/src/policy_catalog.rs; then
  echo "overlay policy catalog is missing the numeric bound" >&2
  record overlay 2
fi
/cargo-home/bin/sha256 file \
  crates/oo-gateway/src/reviewed_policy.rs \
  crates/oo-gateway/src/query_executor.rs \
  crates/oo-gateway/src/bin/query-pg.rs \
  crates/oo-gateway/tests/a327_numeric.rs \
  crates/oo-gateway/src/policy.rs \
  crates/oo-gateway/src/policy_catalog.rs \
  crates/oo-gateway/src/mapping_reader_access.rs \
  Cargo.lock \
  > /evidence/linux/smoke-map/overlay-sha256.txt
record overlay 0

if [ ! -d /cargo-home/a327-clang-root/usr/lib ]; then
  set +e
  bash /extract-libclang.sh > /evidence/linux/smoke-map/libclang-extract.log 2>&1
  extract_exit=$?
  set -e
  record libclang "$extract_exit"
else
  echo "libclang_root=reused" > /evidence/linux/smoke-map/libclang-extract.log
  record libclang 0
fi
if [ ! -s /evidence/linux/smoke-map/libclang-dir.txt ]; then
  find /cargo-home/a327-clang-root/usr/lib \( -name 'libclang.so' -o -name 'libclang-*.so' \) -print -quit | xargs dirname > /evidence/linux/smoke-map/libclang-dir.txt
fi
libdir=$(cat /evidence/linux/smoke-map/libclang-dir.txt)
multiarch=/cargo-home/a327-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a327-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a327-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a327-clang-root/usr/share/pkgconfig"
real=$(find /cargo-home/a327-clang-root/usr/lib -name 'libclang-19.so.19' -print -quit)
devso=$(find /cargo-home/a327-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  export LIBCLANG_PATH="$(dirname "$devso")"
fi
export LD_LIBRARY_PATH="${LIBCLANG_PATH}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
if [ -z "$real" ] || [ ! -f "$llvm_inc/stdbool.h" ]; then
  record libclang-load 1
fi
record libclang-load 0
printf '%s\n' "no" > /evidence/linux/smoke-map/cargo-update-invoked.txt

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked > /evidence/linux/smoke-map/fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"

echo "=== cargo test -p oo-gateway --lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib -- --test-threads=1 \
  > /evidence/linux/smoke-map/test-lib.log 2>&1
lib_exit=$?
set -e
record test-lib "$lib_exit"
if ! grep -q 'bounded_numeric_admits_precision_1_through_38' /evidence/linux/smoke-map/test-lib.log; then
  echo "bounded numeric unit test did not run" >&2
  record test-lib 2
fi

before=16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a
after=$(/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}')
printf 'before=%s\nafter=%s\n' "$before" "$after" > /evidence/linux/smoke-map/lock-hash.txt
if [ "$before" != "$after" ]; then
  record lock-hash 1
fi
record lock-hash 0

printf '%s\n' 0 > /evidence/linux/smoke-map/smoke.exit
echo "smoke=ok"
exit 0
