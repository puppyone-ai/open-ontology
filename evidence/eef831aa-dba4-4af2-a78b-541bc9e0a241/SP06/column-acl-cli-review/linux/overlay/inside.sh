#!/bin/bash
# Overlay proof. Review test, then clippy on the install binary and that test.
# Mapping grants and ANALYZE are inside the test and are not install-column-acl.
# Origin: a235 proof inside.sh 53d597b121eb75b2f21fc4274b9d2c62a38c8f03
# Changes: a241 oracle, query-pg, fixed-commit producer binaries.
set -u
echo "oo_task=sp06-column-acl-cli-review-overlay"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a241"
echo "oo_fixed=13566602dc36ce6ebd03fef25a9bd17aae7a281c"
echo "oo_platform=linux/arm64"
echo "phase=overlay"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
export OO_A241_LIVE=1
unset OO_A235_LIVE || true
mkdir -p /evidence/linux/overlay/phases /evidence/live /cargo-home/bin
cd /src

record() {
  mkdir -p /evidence/linux/overlay/phases
  printf '%s\n' "$2" > "/evidence/linux/overlay/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/overlay/overlay.exit
    exit "$2"
  fi
}

if [ ! -f /evidence/linux/smoke/smoke.exit ] || [ "$(cat /evidence/linux/smoke/smoke.exit)" != "0" ]; then
  echo "pre-overlay smoke exit is not 0" >&2
  record smoke-gate 2
fi
record smoke-gate 0

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/overlay/rustc-version.txt
cargo -V | tee /evidence/linux/overlay/cargo-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
printf '%s\n' 0 > /evidence/linux/overlay/phases/toolchain.exit

if [ ! -x /cargo-home/bin/sha256 ]; then
  rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
  record sha256-tool $?
else
  record sha256-tool 0
fi
export PATH="/cargo-home/bin:${PATH}"

if [ ! -f crates/oo-gateway/tests/install_column_acl_a241_review.rs ]; then
  echo "review test is not mounted" >&2
  record source 2
fi
if [ ! -f crates/oo-gateway/src/bin/install-column-acl.rs ]; then
  echo "install-column-acl is missing" >&2
  record source 2
fi
if [ ! -f crates/oo-gateway/src/bin/query-pg.rs ]; then
  echo "query-pg is missing" >&2
  record source 2
fi
if [ -f rust-toolchain.toml ]; then
  echo "overlay tree still selects a toolchain file" >&2
  record source 2
fi
if [ -e /run/oo-a241/fixture/server.key ] || [ -e /run/oo-a241/fixture/ca.key ] || [ -e /run/oo-a241/fixture/server.csr ]; then
  echo "builder fixture contains a private key" >&2
  record fixture-keys 2
fi
find /run/oo-a241/fixture -type f -print | sort > /evidence/linux/overlay/fixture-files.txt
printf '%s\n' "overlay_mounted" > /evidence/linux/overlay/overlay-mounted.txt
record fixture-keys 0
record source 0

/cargo-home/bin/sha256 file \
  crates/oo-gateway/src/policy.rs \
  crates/oo-gateway/src/lib.rs \
  crates/oo-gateway/Cargo.toml \
  Cargo.lock \
  contracts/gateway/column-acl-set-v1.md \
  contracts/gateway/reviewed-policy-v1.md \
  crates/oo-gateway/src/bin/query-pg.rs \
  crates/oo-gateway/src/bin/reviewed-policy.rs \
  crates/oo-gateway/src/bin/install-column-acl.rs \
  crates/oo-gateway/tests/install_column_acl_cli.rs \
  contracts/gateway/install-column-acl-cli-v1.md \
  > /evidence/linux/overlay/product-sha256.txt
record product-hash $?
while read -r hash path; do
  [ -n "$path" ] || continue
  if ! awk -v h="$hash" -v p="$path" '$1==h && $2==p {found=1} END {exit !found}' /evidence/linux/overlay/product-sha256.txt; then
    echo "frozen file changed $path" >&2
    record frozen-match 1
  fi
done < /evidence/linux/smoke/owned-sha256.txt
record frozen-match 0

libdir=$(cat /evidence/linux/smoke/libclang-dir.txt)
multiarch=/cargo-home/a241-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a241-clang-root/usr/lib/llvm-19/lib/clang/19/include
devso=$(find /cargo-home/a241-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  libdir=$(dirname "$devso")
fi
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a241-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a241-clang-root/usr/share/pkgconfig"
{
  echo "LIBCLANG_PATH=$LIBCLANG_PATH"
  test -f "$llvm_inc/stdbool.h" && echo "stdbool.h=present" || echo "stdbool.h=absent"
} > /evidence/linux/overlay/libclang.txt
if [ ! -f "$llvm_inc/stdbool.h" ]; then
  record libclang 1
fi
record libclang 0
printf '%s\n' "no" > /evidence/linux/overlay/cargo-update-invoked.txt

echo "=== cargo build producer bins ==="
set +e
cargo build --manifest-path Cargo.toml --locked --offline -p oo-builder --bin oo-builder --bin reviewed-effective-oos \
  > /evidence/linux/overlay/producer-build.log 2>&1
producer_build=$?
set -e
record producer-build "$producer_build"
if [ ! -x /target/debug/oo-builder ] || [ ! -x /target/debug/reviewed-effective-oos ]; then
  echo "producer binary missing" >&2
  record producer-bins 1
fi
record producer-bins 0
export OO_A241_OO_BUILDER=/target/debug/oo-builder
export OO_A241_REVIEWED_EFFECTIVE=/target/debug/reviewed-effective-oos

echo "=== cargo test review ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test install_column_acl_a241_review -- --test-threads=1 --nocapture \
  > /evidence/linux/overlay/test.log 2>&1
test_exit=$?
set -e
record test "$test_exit"
if ! grep -q 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out' /evidence/linux/overlay/test.log; then
  echo "expected 1 review test" >&2
  record test-count 1
fi
oracle=/evidence/live/oracle.txt
if [ ! -f "$oracle" ]; then
  echo "live oracle was not written" >&2
  record oracle 1
fi
for fact in \
  'stale=policy stale' \
  'bad_second=policy mapping' \
  'clerk_effect=installed' \
  'replay_effect=replay' \
  'peek_effect=installed' \
  'hidden_write=42501' \
  'mapping_before=0' \
  'query_before_analyze=policy' \
  'query_after_analyze=succeeded' \
  'producer_effect=policy stale'
do
  if ! grep -qx "$fact" "$oracle"; then
    echo "oracle missing $fact" >&2
    record oracle 1
  fi
done
record oracle 0
record test-count 0

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/overlay/clippy-component.log 2>&1
  clippy_add=$?
  set -e
  record clippy-component "$clippy_add"
else
  echo "clippy_component=already_installed" > /evidence/linux/overlay/clippy-component.log
  record clippy-component 0
fi
echo "=== cargo clippy ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline \
  -p oo-gateway --bin install-column-acl --test install_column_acl_a241_review -- -D warnings \
  > /evidence/linux/overlay/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}' > /evidence/linux/overlay/lock-after.sha256
before=$(awk '$2=="Cargo.lock" {print $1}' /evidence/linux/smoke/owned-sha256.txt)
after=$(cat /evidence/linux/overlay/lock-after.sha256)
if [ "$before" != "$after" ]; then
  echo "lock changed before=$before after=$after" > /evidence/linux/overlay/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged $after" > /evidence/linux/overlay/lock-hash.txt
record lock-hash 0

if grep -nE 'unsafe[[:space:]{]' \
  crates/oo-gateway/src/bin/install-column-acl.rs \
  crates/oo-gateway/tests/install_column_acl_a241_review.rs \
  > /evidence/linux/overlay/unsafe-lines.txt
then
  echo "review tree contains an unsafe block" >&2
  record unsafe 2
fi
printf '%s\n' "no unsafe block" > /evidence/linux/overlay/unsafe-lines.txt
printf '%s\n' 0 > /evidence/linux/overlay/phases/unsafe.exit

/cargo-home/bin/sha256 file crates/oo-gateway/tests/install_column_acl_a241_review.rs \
  > /evidence/linux/overlay/review-test-sha256.txt

printf '%s\n' 0 > /evidence/linux/overlay/overlay.exit
echo "overlay_proof=ok"
exit 0
