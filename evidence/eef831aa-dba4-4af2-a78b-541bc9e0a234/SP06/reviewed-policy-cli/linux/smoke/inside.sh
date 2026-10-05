#!/bin/bash
# Pre-edit oo-gateway reviewed-policy smoke on immutable baf70c5c.
# Linux arm64 Rust 1.94. The reviewed-policy CLI is absent. No cargo update. No apt. No PostgreSQL.
# Origin: a227 linux/smoke/inside.sh and linux/retry2/inside.sh.
# Changes: a234 paths, require the adapter, refuse the CLI sources, run the existing 21 adapter tests.
set -u
echo "oo_task=sp06-reviewed-policy-cli-preedit"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a234"
echo "oo_fixed=baf70c5c7b6d01183a178f53c77f65da4a7bb497"
echo "oo_platform=linux/arm64"
echo "phase=preedit"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
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

rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
record sha256-tool $?
rustc --edition 2021 /tools/select-debs.rs -o /cargo-home/bin/select-debs
record select-debs-tool $?
export PATH="/cargo-home/bin:${PATH}"

if [ ! -f crates/oo-gateway/src/reviewed_policy.rs ]; then
  echo "fixed archive is missing reviewed_policy.rs" >&2
  record source 2
fi
if [ -f crates/oo-gateway/src/bin/reviewed-policy.rs ]; then
  echo "archive already contains the reviewed-policy CLI" >&2
  record source 2
fi
if [ -f crates/oo-gateway/tests/reviewed_policy_cli.rs ]; then
  echo "archive already contains the reviewed-policy CLI test" >&2
  record source 2
fi
if [ -f contracts/gateway/reviewed-policy-cli-v1.md ]; then
  echo "archive already contains the reviewed-policy CLI contract" >&2
  record source 2
fi
if [ -f rust-toolchain.toml ]; then
  echo "extracted tree still selects a toolchain file" >&2
  record source 2
fi
printf '%s\n' "cli_absent" > /evidence/linux/smoke/cli-absent.txt

/cargo-home/bin/sha256 file \
  crates/oo-gateway/src/reviewed_policy.rs \
  crates/oo-gateway/src/lib.rs \
  crates/oo-gateway/src/policy.rs \
  crates/oo-gateway/Cargo.toml \
  Cargo.lock \
  > /evidence/linux/smoke/owned-sha256.txt
record owned-hash $?
cp /evidence/linux/smoke/owned-sha256.txt /evidence/linux/smoke/lock-before.sha256

if [ ! -s /evidence/linux/smoke/libclang-dir.txt ] || [ ! -d /cargo-home/a234-clang-root/usr/lib ]; then
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
multiarch=/cargo-home/a234-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a234-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a234-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a234-clang-root/usr/share/pkgconfig"
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
real=$(find /cargo-home/a234-clang-root/usr/lib -name 'libclang-19.so.19' -print -quit)
devso=$(find /cargo-home/a234-clang-root/usr/lib -name 'libclang.so' -print -quit)
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

echo "=== cargo build oo-builder bins ==="
set +e
cargo build --manifest-path Cargo.toml --locked --offline -p oo-builder --bin oo-builder --bin reviewed-effective-oos \
  > /evidence/linux/smoke/build.log 2>&1
build_exit=$?
set -e
record build "$build_exit"
export OO_A227_OO_BUILDER=/target/debug/oo-builder
export OO_A227_REVIEWED_EFFECTIVE=/target/debug/reviewed-effective-oos
if [ ! -x "$OO_A227_OO_BUILDER" ] || [ ! -x "$OO_A227_REVIEWED_EFFECTIVE" ]; then
  echo "builder bins are missing" >&2
  record bins 2
fi
printf '%s\n' "$OO_A227_OO_BUILDER" > /evidence/linux/smoke/bin-oo-builder.txt
printf '%s\n' "$OO_A227_REVIEWED_EFFECTIVE" > /evidence/linux/smoke/bin-reviewed-effective.txt
if [ -x /target/debug/reviewed-policy ]; then
  echo "fixed archive produced a reviewed-policy bin" >&2
  record bins 2
fi
record bins 0

echo "=== cargo test -p oo-gateway --test reviewed_policy ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test reviewed_policy -- --test-threads=1 \
  > /evidence/linux/smoke/test.log 2>&1
test_exit=$?
set -e
record test "$test_exit"
if ! grep -q 'test result: ok. 21 passed' /evidence/linux/smoke/test.log; then
  echo "expected 21 pre-edit adapter tests" >&2
  record test-count 1
fi
record test-count 0

/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}' > /evidence/linux/smoke/lock-after.sha256
before=$(awk '$2 ~ /Cargo.lock$/ {print $1}' /evidence/linux/smoke/owned-sha256.txt)
after=$(cat /evidence/linux/smoke/lock-after.sha256)
if [ "$before" != "$after" ]; then
  echo "lock changed before=$before after=$after" > /evidence/linux/smoke/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged $after" > /evidence/linux/smoke/lock-hash.txt
record lock-hash 0

if grep -n 'unsafe' crates/oo-gateway/src/reviewed_policy.rs crates/oo-gateway/src/policy.rs > /evidence/linux/smoke/unsafe-lines.txt; then
  echo "adapter or policy source contains an unsafe token" >&2
  record unsafe 2
fi
printf '%s\n' 0 > /evidence/linux/smoke/phases/unsafe.exit

printf '%s\n' 0 > /evidence/linux/smoke/smoke.exit
echo "preedit_smoke=ok"
exit 0
