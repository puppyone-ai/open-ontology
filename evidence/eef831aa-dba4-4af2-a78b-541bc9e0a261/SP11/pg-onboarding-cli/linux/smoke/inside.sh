#!/bin/bash
# Focused pre-edit smoke on the immutable 1cd687d3 archive.
# Linux arm64 Rust 1.94. Quantity cell is already in this commit. No cargo update. No apt. No PostgreSQL.
set -u
echo "oo_task=sp11-pg-onboarding-cli-preedit"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a261"
echo "oo_fixed=1cd687d3f106fb9bde8c2a4a0b40cf2dddae9368"
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
rustup show > /evidence/linux/smoke/rustup-show.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
if ! grep -q '1.94.0' /evidence/linux/smoke/rustup-show.txt; then
  echo "rustup show did not report 1.94.0" >&2
  record toolchain 2
fi
record toolchain 0

rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
record sha256-tool $?
printf 'abc' > /cargo-home/sha256-abc.txt
/cargo-home/bin/sha256 file /cargo-home/sha256-abc.txt > /evidence/linux/smoke/sha256-self.txt
record sha256-self $?
if ! grep -q 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad' /evidence/linux/smoke/sha256-self.txt; then
  echo "sha256 abc vector mismatch" >&2
  record sha256-vector 1
fi
record sha256-vector 0
rustc --edition 2021 /tools/select-debs.rs -o /cargo-home/bin/select-debs
record select-debs-tool $?
export PATH="/cargo-home/bin:${PATH}"

sha=$(cat /src/.fixed-base-sha)
printf '%s\n' "$sha" > /evidence/linux/smoke/fixed-base.txt
if [ "$sha" != "1cd687d3f106fb9bde8c2a4a0b40cf2dddae9368" ]; then
  echo "archive sha mismatch" >&2
  record source 2
fi
if [ ! -f contracts/gateway/reviewed-policy-quantity-v1.md ]; then
  echo "quantity contract is absent from the fixed archive" >&2
  record source 2
fi
awk '
  $0 ~ /"int2" \| "int4" \| "int8" => matches!/ {on=1}
  on {print}
  on && $0 ~ /"text" =>/ {exit}
' crates/oo-gateway/src/reviewed_policy.rs > /evidence/linux/smoke/integer-arm.txt
if ! grep -q 'SemanticType::Quantity' /evidence/linux/smoke/integer-arm.txt; then
  echo "fixed archive integer arm does not accept quantity" >&2
  record source 2
fi
if ! grep -q 'SemanticType::StatusCode' /evidence/linux/smoke/integer-arm.txt; then
  echo "fixed archive integer arm lost status code" >&2
  record source 2
fi
printf '%s\n' "quantity_present_in_fixed_archive" > /evidence/linux/smoke/quantity-arm.txt
cmp -s crates/oo-gateway/src/reviewed_policy.rs /evidence/source/blobs/reviewed_policy.rs
record blob-reviewed-policy $?
record source 0

/cargo-home/bin/sha256 file \
  crates/oo-gateway/src/reviewed_policy.rs \
  Cargo.lock \
  > /evidence/linux/smoke/owned-sha256.txt
record owned-hash $?

if [ ! -s /evidence/linux/smoke/libclang-dir.txt ] || [ ! -d /cargo-home/a261-clang-root/usr/lib ]; then
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
multiarch=/cargo-home/a261-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a261-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a261-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a261-clang-root/usr/share/pkgconfig"
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
real=$(find /cargo-home/a261-clang-root/usr/lib -name 'libclang-19.so.19' -print -quit)
devso=$(find /cargo-home/a261-clang-root/usr/lib -name 'libclang.so' -print -quit)
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

run_test() {
  name=$1
  pkg=$2
  testname=$3
  echo "=== cargo test ${pkg} ${testname} ==="
  set +e
  cargo test --manifest-path Cargo.toml --locked --offline -p "$pkg" --test "$testname" -- --test-threads=1 \
    > "/evidence/linux/smoke/test-${testname}.log" 2>&1
  test_exit=$?
  set -e
  record "test-${testname}" "$test_exit"
}

run_test init oo-pipeline init_pg_cli
run_test analyze oo-pipeline analyze_pg_cli
run_test captured oo-builder captured_pg_cli
run_test review oo-builder review_pg_cli
run_test role-pack oo-builder role_pack_cli
run_test role-pack-bundle oo-builder role_pack_bundle
run_test reviewed-policy oo-gateway reviewed_policy_cli
run_test install oo-gateway install_column_acl_cli
run_test provision oo-gateway provision_mapping_reader_cli
run_test query oo-gateway query_pg_cli

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
