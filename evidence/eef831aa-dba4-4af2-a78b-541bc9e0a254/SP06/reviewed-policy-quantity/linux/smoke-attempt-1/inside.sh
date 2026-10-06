#!/bin/bash
# Focused pre-edit smoke on the immutable c828e278 archive.
# Linux arm64 Rust 1.94. No quantity overlay. No cargo update. No apt. No PostgreSQL.
set -u
echo "oo_task=sp06-reviewed-policy-quantity-preedit"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a254"
echo "oo_fixed=c828e2781fc37eb62249fab54fa0b7fff86f0249"
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
mkdir -p /evidence/linux/smoke/phases /cargo-home/bin /cargo-home/a254-bins
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
if [ "$sha" != "c828e2781fc37eb62249fab54fa0b7fff86f0249" ]; then
  echo "archive sha mismatch" >&2
  record source 2
fi
if [ -f crates/oo-gateway/tests/reviewed_policy_quantity.rs ] \
  || [ -f contracts/gateway/reviewed-policy-quantity-v1.md ]; then
  echo "quantity overlay is already on the pre-edit archive" >&2
  record source 2
fi
if ! grep -q 'SemanticType::StatusCode' crates/oo-gateway/src/reviewed_policy.rs; then
  echo "baseline compatible matrix is absent" >&2
  record source 2
fi
if grep -q 'SemanticType::Quantity' crates/oo-gateway/src/reviewed_policy.rs; then
  echo "baseline archive already accepts quantity" >&2
  record source 2
fi
printf '%s\n' "overlay_absent" > /evidence/linux/smoke/overlay-absent.txt
cmp -s crates/oo-gateway/src/reviewed_policy.rs /evidence/source/blobs/reviewed_policy.rs
record blob-reviewed-policy $?
record source 0

/cargo-home/bin/sha256 file \
  crates/oo-gateway/src/reviewed_policy.rs \
  crates/oo-gateway/tests/reviewed_policy.rs \
  crates/oo-gateway/tests/reviewed_policy_cli.rs \
  crates/oo-gateway/src/bin/reviewed-policy.rs \
  Cargo.lock \
  > /evidence/linux/smoke/owned-sha256.txt
record owned-hash $?

if [ ! -s /evidence/linux/smoke/libclang-dir.txt ] || [ ! -d /cargo-home/a254-clang-root/usr/lib ]; then
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
multiarch=/cargo-home/a254-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a254-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a254-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a254-clang-root/usr/share/pkgconfig"
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
real=$(find /cargo-home/a254-clang-root/usr/lib -name 'libclang-19.so.19' -print -quit)
devso=$(find /cargo-home/a254-clang-root/usr/lib -name 'libclang.so' -print -quit)
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

echo "=== producer and reviewed-policy bins ==="
for spec in \
  oo-builder:oo-builder \
  oo-builder:reviewed-effective-oos \
  oo-gateway:reviewed-policy \
  oo-pipeline:oo-pipeline \
  oo-pipeline:init-pg \
  oo-pipeline:analyze-pg \
  oo-gateway:query-pg \
  oo-gateway:install-column-acl
do
  pkg=${spec%%:*}
  bin=${spec##*:}
  set +e
  cargo build --manifest-path Cargo.toml --locked --offline -p "$pkg" --bin "$bin" \
    > "/evidence/linux/smoke/bins-build-${bin}.log" 2>&1
  bins_exit=$?
  set -e
  record "bins-${bin}" "$bins_exit"
done
cp /target/debug/oo-builder /target/debug/reviewed-effective-oos /target/debug/reviewed-policy \
  /target/debug/oo-pipeline /target/debug/init-pg /target/debug/analyze-pg \
  /target/debug/query-pg /target/debug/install-column-acl \
  /cargo-home/a254-bins/
record bin-copy $?

export OO_A227_OO_BUILDER=/cargo-home/a254-bins/oo-builder
export OO_A227_REVIEWED_EFFECTIVE=/cargo-home/a254-bins/reviewed-effective-oos
export OO_A234_OO_BUILDER=/cargo-home/a254-bins/oo-builder
export OO_A234_REVIEWED_EFFECTIVE=/cargo-home/a254-bins/reviewed-effective-oos
export OO_A234_REVIEWED_POLICY=/cargo-home/a254-bins/reviewed-policy

echo "=== existing reviewed_policy tests ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test reviewed_policy -- --test-threads=1 \
  > /evidence/linux/smoke/test-reviewed-policy.log 2>&1
test_exit=$?
set -e
record test-reviewed-policy "$test_exit"
if ! grep -q 'test result: ok. 21 passed' /evidence/linux/smoke/test-reviewed-policy.log; then
  echo "expected 21 pre-edit adapter tests" >&2
  record test-count 1
fi
record test-count 0

echo "=== existing reviewed_policy_cli tests ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test reviewed_policy_cli -- --test-threads=1 \
  > /evidence/linux/smoke/test-reviewed-policy-cli.log 2>&1
cli_exit=$?
set -e
record test-reviewed-policy-cli "$cli_exit"
if ! grep -q 'test result: ok. 6 passed' /evidence/linux/smoke/test-reviewed-policy-cli.log; then
  echo "expected 6 pre-edit CLI tests" >&2
  record cli-count 1
fi
record cli-count 0

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
