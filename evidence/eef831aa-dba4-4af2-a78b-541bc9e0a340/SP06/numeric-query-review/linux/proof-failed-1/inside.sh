#!/bin/bash
# Independent numeric review on the fixed archive. No workspace overlay.
# Origin: evidence/.../a327/SP06/numeric-query/linux/live-4/inside.sh
# sha256 b6279111787bd82fb5db8b2763c10545bda5ae8f0b47827b223aa1ccdfd5affc
# Changes: a340 names, archive hashes instead of a file copy, gold oracle,
# clippy recorded before the live chain. Rust 1.94. No cargo update.
set -u
echo "oo_task=sp06-numeric-query-review"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a340"
echo "oo_fixed=21cc96241a841b47dbbcb116549030e38a643ba0"
echo "oo_platform=linux/arm64"
echo "overlay=none"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export RUSTUP_HOME=/usr/local/rustup
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/a340
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
export PATH="/usr/local/cargo/bin:${PATH}"
mkdir -p /evidence/linux/proof/phases /evidence/linux/proof/steps /cargo-home/bin
cd /src
unset OO_A327_LIVE
CLIPPY_FAIL=0

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
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
record toolchain 0

if [ ! -x /cargo-home/bin/sha256 ]; then
  rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
fi
export PATH="/cargo-home/bin:${PATH}"

/cargo-home/bin/sha256 file \
  crates/oo-gateway/src/reviewed_policy.rs \
  crates/oo-gateway/src/query_executor.rs \
  crates/oo-gateway/src/bin/query-pg.rs \
  crates/oo-gateway/tests/a327_numeric.rs \
  crates/oo-gateway/src/policy.rs \
  crates/oo-gateway/src/policy_catalog.rs \
  crates/oo-gateway/src/mapping_reader_access.rs \
  Cargo.lock \
  > /evidence/linux/proof/archive-sha256.txt
if ! cmp -s /evidence/linux/proof/archive-sha256.txt /evidence/source/expected.sha256; then
  echo "fixed archive hashes do not match the committed seven sources and lock" >&2
  record archive-hash 2
fi
if ! grep -q 'fn grantable_base_type' crates/oo-gateway/src/policy_catalog.rs; then
  record archive-hash 2
fi
if ! grep -q 'fn live_numeric_proof' crates/oo-gateway/src/bin/query-pg.rs; then
  record archive-hash 2
fi
if grep -q 'SUPPORTED_TYPES.contains' crates/oo-gateway/src/mapping_reader_access.rs; then
  echo "mapping reader still checks the type name only" >&2
  record archive-hash 2
fi
printf '%s\n' none > /evidence/linux/proof/overlay.txt
record archive-hash 0

if [ ! -d /cargo-home/a340-clang-root/usr/lib ]; then
  set +e
  bash /extract-libclang.sh > /evidence/linux/proof/libclang-extract.log 2>&1
  extract_exit=$?
  set -e
  record libclang "$extract_exit"
else
  echo "libclang_root=reused" > /evidence/linux/proof/libclang-extract.log
  record libclang 0
fi
if [ ! -s /evidence/linux/proof/libclang-dir.txt ]; then
  find /cargo-home/a340-clang-root/usr/lib \( -name 'libclang.so' -o -name 'libclang-*.so' \) -print -quit | xargs dirname > /evidence/linux/proof/libclang-dir.txt
fi
libdir=$(cat /evidence/linux/proof/libclang-dir.txt)
multiarch=/cargo-home/a340-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a340-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/usr/local/cargo/bin:/cargo-home/a340-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a340-clang-root/usr/share/pkgconfig"
real=$(find /cargo-home/a340-clang-root/usr/lib -name 'libclang-19.so.19' -print -quit)
devso=$(find /cargo-home/a340-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  export LIBCLANG_PATH="$(dirname "$devso")"
fi
export LD_LIBRARY_PATH="${LIBCLANG_PATH}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
if [ -z "$real" ] || [ ! -f "$llvm_inc/stdbool.h" ]; then
  record libclang-load 1
fi
record libclang-load 0
printf '%s\n' "no" > /evidence/linux/proof/cargo-update-invoked.txt

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/proof/clippy-component.log 2>&1
  clippy_add=$?
  set -e
  record clippy-component "$clippy_add"
else
  echo "clippy_component=already_installed" > /evidence/linux/proof/clippy-component.log
  record clippy-component 0
fi

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked > /evidence/linux/proof/fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"

echo "=== preoverlay cargo test -p oo-gateway --lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib -- --test-threads=1 \
  > /evidence/linux/proof/test-lib.log 2>&1
lib_exit=$?
set -e
record test-lib "$lib_exit"

echo "=== preoverlay cargo test query-pg decimal_json ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --bin query-pg decimal_json -- --test-threads=1 \
  > /evidence/linux/proof/test-query-pg.log 2>&1
bin_exit=$?
set -e
record test-query-pg "$bin_exit"

echo "=== preoverlay cargo test query-pg absent_row_filter_null ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --bin query-pg absent_row_filter_null -- --test-threads=1 \
  > /evidence/linux/proof/test-row-filter.log 2>&1
row_exit=$?
set -e
record test-row-filter "$row_exit"

echo "=== preoverlay cargo clippy -p oo-gateway --lib --bins ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib --bins -- -D warnings \
  > /evidence/linux/proof/clippy.log 2>&1
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/linux/proof/phases/clippy.exit
echo "phase_exit=$clippy_exit phase=clippy"
if [ "$clippy_exit" -ne 0 ]; then
  CLIPPY_FAIL=$clippy_exit
fi

echo "=== cargo build pipeline bins ==="
set +e
cargo build --manifest-path Cargo.toml --locked --offline -p oo-pipeline --bin oo-pipeline --bin init-pg --bin analyze-pg \
  > /evidence/linux/proof/build-pipeline.log 2>&1
pipe_exit=$?
set -e
record build-pipeline "$pipe_exit"

echo "=== cargo build builder bins ==="
set +e
cargo build --manifest-path Cargo.toml --locked --offline -p oo-builder --bin oo-builder --bin reviewed-effective-oos --bin role-pack-bundle \
  > /evidence/linux/proof/build-builder.log 2>&1
builder_exit=$?
set -e
record build-builder "$builder_exit"

echo "=== cargo build gateway bins ==="
set +e
cargo build --manifest-path Cargo.toml --locked --offline -p oo-gateway --bin reviewed-policy --bin install-column-acl --bin provision-mapping-reader --bin query-pg \
  > /evidence/linux/proof/build-gateway.log 2>&1
gateway_exit=$?
set -e
record build-gateway "$gateway_exit"

echo "=== live numeric proof ==="
rm -f /target/a340/a327-role-pack-bundle.json /target/a340/a327-role-pack-bundle.json.role-pack-bundle-staging
rm -f /target/a327/a327-role-pack-bundle.json /target/a327/a327-role-pack-bundle.json.role-pack-bundle-staging
export OO_A327_LIVE=1
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --bin query-pg -- --exact a327_live::live_numeric_proof --nocapture --test-threads=1 \
  > /evidence/linux/proof/test-live.log 2>&1
live_exit=$?
set -e
if [ "$live_exit" -ne 0 ]; then
  record test-live "$live_exit"
fi
if ! grep -q 'test a327_live::live_numeric_proof ... ok' /evidence/linux/proof/test-live.log; then
  echo "live test did not record a pass" >&2
  record test-live 2
fi
if [ ! -s /evidence/linux/proof/outcome.txt ]; then
  echo "live outcome was not written" >&2
  record test-live 2
fi
record test-live 0

echo "=== independent gold ==="
rustc --edition 2021 /tools/gold.rs -o /cargo-home/bin/gold
set +e
/cargo-home/bin/gold
gold_exit=$?
set -e
record gold "$gold_exit"

before=b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474
after=$(/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}')
printf 'before=%s\nafter=%s\nauthor_overlay_lock_not_used=16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a\n' "$before" "$after" > /evidence/linux/proof/lock-hash.txt
if [ "$before" != "$after" ]; then
  record lock-hash 1
fi
record lock-hash 0

if [ "$CLIPPY_FAIL" -ne 0 ]; then
  printf '%s\n' "$CLIPPY_FAIL" > /evidence/linux/proof/proof.exit
  echo "proof=clippy_failed"
  exit "$CLIPPY_FAIL"
fi
printf '%s\n' 0 > /evidence/linux/proof/proof.exit
echo "proof=ok"
exit 0
