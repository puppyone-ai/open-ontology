#!/bin/bash
# Pre-edit live run. The old assertion is expected to fail. Do not treat that as a harness bug.
# Linux arm64 Rust 1.94. Current committed executor and catalog. No cargo update.
set -u
echo "oo_task=sp06-catalog-test-alignment-red"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a206"
echo "oo_fixed=5e3ba29f8d460a1aa6919733100c34a68a2fdb75"
echo "oo_platform=linux/arm64"
echo "phase=preedit-live"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=4
export OO_A190_LIVE=1
mkdir -p /evidence/linux/red/phases /evidence/live /cargo-home/bin
cd /src

phase() {
  mkdir -p /evidence/linux/red/phases
  printf '%s\n' "$2" > "/evidence/linux/red/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/red/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) phase toolchain 0 ;;
  *) echo "refusing toolchain: $got" >&2; phase toolchain 2; exit 2 ;;
esac

if [ ! -x /cargo-home/bin/sha256 ]; then
  rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
  phase sha256-tool $?
else
  phase sha256-tool 0
fi
export PATH="/cargo-home/bin:${PATH}"

if ! grep -q 'assert_eq!(session.bound_role(), None);' crates/oo-gateway/tests/policy_catalog_pg_live.rs; then
  echo "red archive does not contain the old assertion" >&2
  phase old-assertion 2
  exit 2
fi
phase old-assertion 0

/cargo-home/bin/sha256 file \
  crates/oo-gateway/src/policy.rs \
  crates/oo-gateway/src/policy_catalog.rs \
  crates/oo-gateway/src/query_executor.rs \
  crates/oo-gateway/tests/policy_catalog_pg_live.rs \
  contracts/gateway/policy-catalog-v1.md \
  Cargo.lock \
  > /evidence/linux/red/product-sha256.txt
mismatch=0
while read -r digest rel; do
  [ -n "$rel" ] || continue
  case "$rel" in
    crates/oo-gateway/Cargo.toml) continue ;;
  esac
  want=$(awk -v rel="$rel" '$2==rel {print $1}' /evidence/linux/smoke/owned-sha256.txt)
  if [ "$want" != "$digest" ]; then
    echo "mismatch $rel smoke=$want red=$digest" >> /evidence/linux/red/product-match.txt
    mismatch=1
  fi
done < /evidence/linux/red/product-sha256.txt
if [ "$mismatch" -ne 0 ]; then
  phase product-match 1
  exit 1
fi
echo "match" > /evidence/linux/red/product-match.txt
phase product-match 0

libdir=$(cat /evidence/linux/smoke/libclang-dir.txt)
multiarch=/cargo-home/a206-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a206-clang-root/usr/lib/llvm-19/lib/clang/19/include
devso=$(find /cargo-home/a206-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  libdir=$(dirname "$devso")
fi
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a206-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a206-clang-root/usr/share/pkgconfig"
if [ ! -f "$llvm_inc/stdbool.h" ] || [ -z "$libdir" ]; then
  phase libclang 1
  exit 1
fi
phase libclang 0
printf '%s\n' "no" > /evidence/linux/red/cargo-update-invoked.txt

echo "=== OO_A190_LIVE=1 old assertion ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline \
  -p oo-gateway --test policy_catalog_pg_live -- --test-threads=1 --nocapture \
  > /evidence/linux/red/test.log 2>&1
test_exit=$?
set -e
phase live-test "$test_exit"
if [ -f /evidence/live/oracle.txt ]; then
  cp /evidence/live/oracle.txt /evidence/live/red-oracle.txt
fi
if [ "$test_exit" -eq 0 ]; then
  echo "unexpected pass" > /evidence/linux/red/reproduced.txt
  phase reproduced 1
  exit 1
fi
if grep -Eq 'assertion `left == right` failed' /evidence/linux/red/test.log \
  && grep -Eq 'left: Some\("oo_r_clerk"\)' /evidence/linux/red/test.log \
  && grep -Eq 'right: None' /evidence/linux/red/test.log \
  && grep -Eq 'policy_catalog_pg_live.rs:575' /evidence/linux/red/test.log
then
  echo "yes line=575 left=Some(oo_r_clerk) right=None exit=${test_exit}" > /evidence/linux/red/reproduced.txt
  phase reproduced 0
  printf '%s\n' 0 > /evidence/linux/red/red.exit
  echo "preedit_assertion_reproduced=yes"
  exit 0
fi
echo "no" > /evidence/linux/red/reproduced.txt
phase reproduced 1
printf '%s\n' 1 > /evidence/linux/red/red.exit
echo "preedit_assertion_reproduced=no" >&2
exit 1
