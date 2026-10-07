#!/bin/bash
# Mapping-reader numeric bound proof. Linux arm64 Rust 1.94. No cargo update.
# Origin: evidence/.../numeric-query/linux/live-3/inside.sh
# Changes: overlay mapping_reader_access.rs, evidence directory linux/live-4.
set -u
echo "oo_task=sp06-numeric-query-live-4"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a327"
echo "oo_fixed=e6ebf93b5ac261c097a24d145611e0300e4f88b5"
echo "oo_platform=linux/arm64"
echo "phase=live-4"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/a327
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /evidence/linux/live-4/phases /evidence/linux/live-4/steps /cargo-home/bin
cd /src
unset OO_A327_LIVE

record() {
  mkdir -p /evidence/linux/live-4/phases
  printf '%s\n' "$2" > "/evidence/linux/live-4/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/live-4/live.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/live-4/rustc-version.txt
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
if ! grep -q 'fn live_numeric_proof' crates/oo-gateway/src/bin/query-pg.rs; then
  echo "overlay query-pg is missing the live proof" >&2
  record overlay 2
fi
if ! grep -q 'fn grantable_base_type' crates/oo-gateway/src/policy_catalog.rs; then
  echo "overlay policy catalog is missing the numeric bound" >&2
  record overlay 2
fi
if ! grep -q 'grantable_base_type' crates/oo-gateway/src/policy.rs; then
  echo "overlay policy grant is missing the numeric bound" >&2
  record overlay 2
fi
if ! grep -q 'grantable_base_type' crates/oo-gateway/src/mapping_reader_access.rs; then
  echo "overlay mapping reader is missing the numeric bound" >&2
  record overlay 2
fi
if grep -q 'SUPPORTED_TYPES.contains' crates/oo-gateway/src/mapping_reader_access.rs; then
  echo "overlay mapping reader still checks the type name only" >&2
  record overlay 2
fi
if ! grep -q 'provision-loose' crates/oo-gateway/src/bin/query-pg.rs; then
  echo "overlay query-pg is missing the unbound numeric refusal" >&2
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
  > /evidence/linux/live-4/overlay-sha256.txt
record overlay 0

if [ ! -d /cargo-home/a327-clang-root/usr/lib ]; then
  set +e
  bash /extract-libclang.sh > /evidence/linux/live-4/libclang-extract.log 2>&1
  extract_exit=$?
  set -e
  record libclang "$extract_exit"
else
  echo "libclang_root=reused" > /evidence/linux/live-4/libclang-extract.log
  record libclang 0
fi
if [ ! -s /evidence/linux/live-4/libclang-dir.txt ]; then
  find /cargo-home/a327-clang-root/usr/lib \( -name 'libclang.so' -o -name 'libclang-*.so' \) -print -quit | xargs dirname > /evidence/linux/live-4/libclang-dir.txt
fi
libdir=$(cat /evidence/linux/live-4/libclang-dir.txt)
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
printf '%s\n' "no" > /evidence/linux/live-4/cargo-update-invoked.txt

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/live-4/clippy-component.log 2>&1
  clippy_add=$?
  set -e
  record clippy-component "$clippy_add"
else
  echo "clippy_component=already_installed" > /evidence/linux/live-4/clippy-component.log
  record clippy-component 0
fi

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked > /evidence/linux/live-4/fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"

echo "=== cargo test -p oo-gateway --lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib -- --test-threads=1 \
  > /evidence/linux/live-4/test-lib.log 2>&1
lib_exit=$?
set -e
record test-lib "$lib_exit"
if ! grep -q 'bounded_numeric_admits_precision_1_through_38' /evidence/linux/live-4/test-lib.log; then
  echo "bounded numeric unit test did not run" >&2
  record test-lib 2
fi

echo "=== cargo test -p oo-gateway --bin query-pg decimal_json ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --bin query-pg decimal_json -- --test-threads=1 \
  > /evidence/linux/live-4/test-query-pg.log 2>&1
bin_exit=$?
set -e
record test-query-pg "$bin_exit"

echo "=== cargo test -p oo-gateway --bin query-pg absent_row_filter_null ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --bin query-pg absent_row_filter_null -- --test-threads=1 \
  > /evidence/linux/live-4/test-row-filter.log 2>&1
row_exit=$?
set -e
record test-row-filter "$row_exit"

echo "=== cargo clippy -p oo-gateway --lib --bins ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib --bins -- -D warnings \
  > /evidence/linux/live-4/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

echo "=== cargo build pipeline bins ==="
set +e
cargo build --manifest-path Cargo.toml --locked --offline -p oo-pipeline --bin oo-pipeline --bin init-pg --bin analyze-pg \
  > /evidence/linux/live-4/build-pipeline.log 2>&1
pipe_exit=$?
set -e
record build-pipeline "$pipe_exit"

echo "=== cargo build builder bins ==="
set +e
cargo build --manifest-path Cargo.toml --locked --offline -p oo-builder --bin oo-builder --bin reviewed-effective-oos --bin role-pack-bundle \
  > /evidence/linux/live-4/build-builder.log 2>&1
builder_exit=$?
set -e
record build-builder "$builder_exit"

echo "=== cargo build gateway bins ==="
set +e
cargo build --manifest-path Cargo.toml --locked --offline -p oo-gateway --bin reviewed-policy --bin install-column-acl --bin provision-mapping-reader --bin query-pg \
  > /evidence/linux/live-4/build-gateway.log 2>&1
gateway_exit=$?
set -e
record build-gateway "$gateway_exit"

echo "=== live numeric proof ==="
rm -f /target/a327/a327-role-pack-bundle.json /target/a327/a327-role-pack-bundle.json.role-pack-bundle-staging
export OO_A327_LIVE=1
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --bin query-pg -- --exact a327_live::live_numeric_proof --nocapture --test-threads=1 \
  > /evidence/linux/live-4/test-live.log 2>&1
live_exit=$?
set -e
if [ "$live_exit" -ne 0 ]; then
  record test-live "$live_exit"
fi
if ! grep -q 'test a327_live::live_numeric_proof ... ok' /evidence/linux/live-4/test-live.log; then
  echo "live test did not record a pass" >&2
  record test-live 2
fi
if [ ! -s /evidence/linux/live-4/outcome.txt ]; then
  echo "live outcome was not written" >&2
  record test-live 2
fi
record test-live 0

before=16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a
after=$(/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}')
printf 'before=%s\nafter=%s\n' "$before" "$after" > /evidence/linux/live-4/lock-hash.txt
if [ "$before" != "$after" ]; then
  record lock-hash 1
fi
record lock-hash 0

printf '%s\n' 0 > /evidence/linux/live-4/live.exit
echo "live=ok"
exit 0
