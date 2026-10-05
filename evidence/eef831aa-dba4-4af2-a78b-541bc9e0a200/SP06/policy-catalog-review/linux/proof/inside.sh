#!/bin/bash
# Live policy-catalog review on fixed 78a60fde plus the mounted a200 overlay.
# Linux arm64 Rust 1.94. One test binary. No cargo update. No apt.
set -u
echo "oo_task=sp06-policy-catalog-review-proof"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a200"
echo "oo_fixed=78a60fdeabcada0d024fc6f9a81e0e6b184260e8"
echo "oo_platform=linux/arm64"
echo "phase=proof"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=4
export OO_A200_LIVE=1
mkdir -p /evidence/linux/proof/phases /cargo-home/bin
cd /src

record() {
  mkdir -p /evidence/linux/proof/phases
  printf '%s\n' "$2" > "/evidence/linux/proof/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/proof/proof.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/proof/rustc-version.txt
cargo -V | tee /evidence/linux/proof/cargo-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac

if [ ! -x /cargo-home/bin/sha256 ]; then
  rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
  record sha256-tool $?
else
  record sha256-tool 0
fi
export PATH="/cargo-home/bin:${PATH}"

if [ ! -f crates/oo-gateway/src/policy_catalog.rs ]; then
  echo "fixed archive is missing policy_catalog.rs" >&2
  record source 2
fi
if [ ! -f crates/oo-gateway/tests/policy_catalog_a200_review.rs ]; then
  echo "review overlay is not mounted" >&2
  record source 2
fi
printf '%s\n' "overlay_mounted" > /evidence/linux/proof/overlay-mounted.txt

/cargo-home/bin/sha256 file \
  crates/oo-gateway/src/policy_catalog.rs \
  crates/oo-gateway/src/policy.rs \
  crates/oo-gateway/src/query_executor.rs \
  crates/oo-gateway/src/sql_guard.rs \
  crates/oo-gateway/tests/policy_catalog_pg_live.rs \
  contracts/gateway/policy-catalog-v1.md \
  contracts/gateway/pg-policy-v1.md \
  contracts/gateway/query-executor-v1.md \
  Cargo.lock \
  crates/oo-gateway/Cargo.toml \
  > /evidence/linux/proof/product-sha256.txt
record product-hash $?
if ! cmp -s /evidence/linux/proof/product-sha256.txt /evidence/linux/smoke/owned-sha256.txt; then
  echo "product bytes differ from the pre-overlay smoke" >&2
  record product-match 1
fi
record product-match 0
/cargo-home/bin/sha256 file crates/oo-gateway/tests/policy_catalog_a200_review.rs \
  > /evidence/linux/proof/overlay.sha256
record overlay-hash $?

libdir=$(cat /evidence/linux/smoke/libclang-dir.txt)
multiarch=/cargo-home/a200-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a200-clang-root/usr/lib/llvm-19/lib/clang/19/include
devso=$(find /cargo-home/a200-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  libdir=$(dirname "$devso")
fi
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a200-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a200-clang-root/usr/share/pkgconfig"
{
  echo "LIBCLANG_PATH=$LIBCLANG_PATH"
  test -f "$llvm_inc/stdbool.h" && echo "stdbool.h=present" || echo "stdbool.h=absent"
} > /evidence/linux/proof/libclang.txt
if [ ! -f "$llvm_inc/stdbool.h" ] || [ -z "$libdir" ]; then
  record libclang 1
fi
record libclang 0
printf '%s\n' "no" > /evidence/linux/proof/cargo-update-invoked.txt

echo "=== cargo test policy_catalog_a200_review ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline \
  -p oo-gateway --test policy_catalog_a200_review -- --test-threads=1 --nocapture \
  > /evidence/linux/proof/test.log 2>&1
test_exit=$?
set -e
record test "$test_exit"

/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}' > /evidence/linux/proof/lock-after.sha256
before=$(awk '$2 ~ /Cargo.lock$/ {print $1}' /evidence/linux/smoke/owned-sha256.txt)
after=$(cat /evidence/linux/proof/lock-after.sha256)
if [ "$before" != "$after" ]; then
  echo "lock changed before=$before after=$after" > /evidence/linux/proof/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged $after" > /evidence/linux/proof/lock-hash.txt
record lock-hash 0

printf '%s\n' 0 > /evidence/linux/proof/proof.exit
echo "proof=ok"
exit 0
