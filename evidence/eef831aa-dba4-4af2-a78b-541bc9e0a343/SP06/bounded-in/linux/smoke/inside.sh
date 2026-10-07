#!/bin/bash
# Pre-edit smoke of archive 21cc962. No product overlay. No cargo update.
# Linux arm64 Rust 1.94. One CPU. Existing sql_guard tests only.
set -u
echo "oo_task=sp06-bounded-in-preedit-smoke"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a343"
echo "oo_fixed=21cc96241a841b47dbbcb116549030e38a643ba0"
echo "oo_platform=linux/arm64"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/a343-smoke
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /evidence/linux/smoke/phases /cargo-home/bin
cd /src

record() {
  mkdir -p /evidence/linux/smoke/phases
  printf '%s\n' "$2" > "/evidence/linux/smoke/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/smoke/smoke.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/smoke/rustc-version.txt
rustc -Vv > /evidence/linux/smoke/rustc-verbose.txt
cargo -V | tee /evidence/linux/smoke/cargo-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
record toolchain 0

if [ ! -f crates/oo-gateway/src/sql_guard.rs ]; then
  echo "archive is missing sql_guard.rs" >&2
  record source 2
fi
if [ -f crates/oo-gateway/tests/bounded_in.rs ]; then
  echo "pre-edit archive already contains bounded_in.rs" >&2
  record source 2
fi
printf '%s\n' "overlay=absent" > /evidence/linux/smoke/overlay-absent.txt
record source 0

rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
record sha256-tool $?
rustc --edition 2021 /tools/select-debs.rs -o /cargo-home/bin/select-debs
record select-debs-tool $?
export PATH="/cargo-home/bin:${PATH}"

/cargo-home/bin/sha256 file crates/oo-gateway/src/sql_guard.rs Cargo.lock crates/oo-gateway/Cargo.toml \
  > /evidence/linux/smoke/owned-sha256.txt
record owned-hash $?
want=1db8f3ed11a2d816d485bdd2865f37aff3e5cbed
got_blob=$(git -C /src hash-object crates/oo-gateway/src/sql_guard.rs 2>/dev/null || true)
if [ -z "$got_blob" ]; then
  got_blob=$(/cargo-home/bin/sha256 file crates/oo-gateway/src/sql_guard.rs | awk '{print $1}')
  echo "git hash-object unavailable; sha256 recorded instead of blob id" > /evidence/linux/smoke/blob-note.txt
fi
printf '%s\n' "$got_blob" > /evidence/linux/smoke/sql-guard-blob.txt
# Blob id is checked on the host. This file records the in-container digest too.
record blob 0

if [ ! -s /evidence/linux/smoke/libclang-dir.txt ] || [ ! -d /cargo-home/a343-clang-root/usr/lib ]; then
  set +e
  bash /extract-libclang.sh > /evidence/linux/smoke/libclang-extract.log 2>&1
  extract_exit=$?
  set -e
  record libclang "$extract_exit"
else
  echo "libclang_root=reused" > /evidence/linux/smoke/libclang-extract.log
  record libclang 0
fi
libdir=$(cat /evidence/linux/smoke/libclang-dir.txt)
multiarch=/cargo-home/a343-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a343-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a343-clang-root/usr/bin:${PATH}"
{
  echo "LIBCLANG_PATH=$LIBCLANG_PATH"
  echo "LD_LIBRARY_PATH=$LD_LIBRARY_PATH"
  ls -l "$libdir"/libclang.so* || true
  test -f "$llvm_inc/stdbool.h" && echo "stdbool.h=present" || echo "stdbool.h=absent"
} > /evidence/linux/smoke/libclang.txt
if [ ! -f "$llvm_inc/stdbool.h" ]; then
  record libclang-includes 1
fi
record libclang-includes 0
real=$(find /cargo-home/a343-clang-root/usr/lib -name 'libclang-19.so.19' -print -quit)
devso=$(find /cargo-home/a343-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  export LIBCLANG_PATH="$(dirname "$devso")"
fi
export LD_LIBRARY_PATH="${LIBCLANG_PATH}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
if [ -z "$real" ]; then
  echo "libclang-19.so.19 missing under the private clang root" >&2
  record libclang-load 1
fi
printf '%s\n' "$LIBCLANG_PATH" > /evidence/linux/smoke/libclang-path.txt
/lib/ld-linux-aarch64.so.1 --list "$real" > /evidence/linux/smoke/libclang-ldd.txt 2>&1 || true
if grep -Eq 'not found|error while loading' /evidence/linux/smoke/libclang-ldd.txt; then
  record libclang-load 1
fi
if ! grep -q 'libLLVM' /evidence/linux/smoke/libclang-ldd.txt; then
  echo "libclang did not resolve LLVM" >&2
  record libclang-load 1
fi
record libclang-load 0
printf '%s\n' "no" > /evidence/linux/smoke/cargo-update-invoked.txt

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked > /evidence/linux/smoke/fetch.log 2>&1
fetch_exit=$?
set -e
if grep -n 'cargo update' /evidence/linux/smoke/fetch.log; then
  echo "fetch log mentioned cargo update" >&2
  record fetch 2
fi
record fetch "$fetch_exit"

echo "=== cargo test -p oo-gateway --test sql_guard ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test sql_guard -- --test-threads=1 \
  > /evidence/linux/smoke/test.log 2>&1
test_exit=$?
set -e
record test "$test_exit"

/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}' > /evidence/linux/smoke/lock-after.sha256
before=$(awk '$2 ~ /Cargo.lock$/ {print $1}' /evidence/linux/smoke/owned-sha256.txt)
after=$(cat /evidence/linux/smoke/lock-after.sha256)
if [ "$before" != "$after" ]; then
  echo "lock changed before=$before after=$after" > /evidence/linux/smoke/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged $after" > /evidence/linux/smoke/lock-hash.txt
record lock-hash 0

printf '%s\n' 0 > /evidence/linux/smoke/smoke.exit
echo "preedit_smoke=ok"
exit 0
