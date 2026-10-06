#!/bin/bash
# Pre-overlay smoke of fixed commit 1f1028f6. Linux arm64 Rust 1.94.
# The review test is not on this tree. Live environment variables are unset.
# Origin: a241 smoke inside.sh 8fe6963b17c561e520fffff1c443c8a0d28338c9
# which followed a235 smoke inside.sh 3910361e5c8c90fd584687e68ed7a32a707560aa.
# Changes: a246 paths, mapping_reader_access unit tests, gated live test, no review file.
set -u
echo "oo_task=sp06-mapping-reader-review-preoverlay"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a246"
echo "oo_fixed=1f1028f6161783d6e3bf1e9beb3a23a1161deca0"
echo "oo_platform=linux/arm64"
echo "phase=preoverlay"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
unset OO_A240_LIVE || true
unset OO_A246_LIVE || true
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
rustup show > /evidence/linux/smoke/rustup-show.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
printf '%s\n' 0 > /evidence/linux/smoke/phases/toolchain.exit

rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
record sha256-tool $?
rustc --edition 2021 /tools/select-debs.rs -o /cargo-home/bin/select-debs
record select-debs-tool $?
export PATH="/cargo-home/bin:${PATH}"

if [ ! -f crates/oo-gateway/src/mapping_reader_access.rs ]; then
  echo "fixed archive is missing mapping_reader_access.rs" >&2
  record source 2
fi
if [ ! -f crates/oo-gateway/tests/mapping_reader_access_pg_live.rs ]; then
  echo "fixed archive is missing the committed live test" >&2
  record source 2
fi
if [ ! -f contracts/gateway/mapping-reader-access-v1.md ]; then
  echo "fixed archive is missing the mapping reader contract" >&2
  record source 2
fi
if [ -f crates/oo-gateway/tests/mapping_reader_access_a246_review.rs ]; then
  echo "review overlay is present before the overlay phase" >&2
  record source 2
fi
if [ -f rust-toolchain.toml ]; then
  echo "extracted tree still selects a toolchain file" >&2
  record source 2
fi
printf '%s\n' "overlay_absent" > /evidence/linux/smoke/overlay-absent.txt
record source 0

/cargo-home/bin/sha256 file \
  crates/oo-gateway/src/mapping_reader_access.rs \
  crates/oo-gateway/src/lib.rs \
  crates/oo-gateway/tests/mapping_reader_access_pg_live.rs \
  contracts/gateway/mapping-reader-access-v1.md \
  crates/oo-gateway/src/policy.rs \
  Cargo.lock \
  crates/oo-pipeline/src/bin/init-pg.rs \
  crates/oo-pipeline/src/bin/oo-pipeline.rs \
  > /evidence/linux/smoke/owned-sha256.txt
record owned-hash $?

if [ ! -s /evidence/linux/smoke/libclang-dir.txt ] || [ ! -d /cargo-home/a246-clang-root/usr/lib ]; then
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
multiarch=/cargo-home/a246-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a246-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a246-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a246-clang-root/usr/share/pkgconfig"
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
real=$(find /cargo-home/a246-clang-root/usr/lib -name 'libclang-19.so.19' -print -quit)
devso=$(find /cargo-home/a246-clang-root/usr/lib -name 'libclang.so' -print -quit)
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
record fetch "$fetch_exit"

echo "=== cargo test -p oo-gateway --lib mapping_reader_access ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib mapping_reader_access -- --test-threads=1 \
  > /evidence/linux/smoke/test-lib.log 2>&1
lib_exit=$?
set -e
record test-lib "$lib_exit"
if ! grep -q 'test result: ok. 5 passed; 0 failed;' /evidence/linux/smoke/test-lib.log; then
  echo "expected 5 pre-overlay mapping reader unit tests" >&2
  record test-count 1
fi

echo "=== cargo test -p oo-gateway --test mapping_reader_access_pg_live (live unset) ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test mapping_reader_access_pg_live -- --test-threads=1 \
  > /evidence/linux/smoke/test-live-gate.log 2>&1
gate_exit=$?
set -e
record test-live-gate "$gate_exit"
if ! grep -q 'test result: ok. 1 passed; 0 failed;' /evidence/linux/smoke/test-live-gate.log; then
  echo "expected the committed live test to return before a connection" >&2
  record test-count 1
fi
if [ -f /evidence/live/oracle.txt ]; then
  echo "pre-overlay run wrote a live oracle" >&2
  record test-count 1
fi
record test-count 0

/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}' > /evidence/linux/smoke/lock-after.sha256
before=$(awk '$2=="Cargo.lock" {print $1}' /evidence/linux/smoke/owned-sha256.txt)
after=$(cat /evidence/linux/smoke/lock-after.sha256)
if [ "$before" != "$after" ]; then
  echo "lock changed before=$before after=$after" > /evidence/linux/smoke/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged $after" > /evidence/linux/smoke/lock-hash.txt
record lock-hash 0

if grep -nE 'unsafe[[:space:]{]' \
  crates/oo-gateway/src/mapping_reader_access.rs \
  crates/oo-gateway/tests/mapping_reader_access_pg_live.rs \
  > /evidence/linux/smoke/unsafe-lines.txt; then
  echo "committed mapping reader contains an unsafe block" >&2
  record unsafe 2
fi
printf '%s\n' "no unsafe block" > /evidence/linux/smoke/unsafe-lines.txt
printf '%s\n' 0 > /evidence/linux/smoke/phases/unsafe.exit

printf '%s\n' 0 > /evidence/linux/smoke/smoke.exit
echo "preoverlay_smoke=ok"
exit 0
