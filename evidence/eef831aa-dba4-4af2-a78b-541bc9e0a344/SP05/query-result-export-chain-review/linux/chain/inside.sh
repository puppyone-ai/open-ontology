#!/bin/bash
# Build query-pg from the 21cc9624 archive, run the frozen live numeric proof,
# then export that stdout with the frozen 62b4afec exporter.
# Producer unit tests and producer Clippy are not repeated. Rust 1.94.
# No cargo update. No moving source.
# Origin: a333 actual-pg-chain linux/chain/inside.sh
# SHA-256 292a700bb0a323de575e51738f46158f108c41dd676378b0121319e2631171a9
# Changes: a344 evidence paths and cargo target. The bundle path stays
# /target/a327 because the frozen live test writes that path.
set -u
echo "oo_task=sp05-query-result-export-chain-review"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a344"
echo "oo_producer=21cc96241a841b47dbbcb116549030e38a643ba0"
echo "oo_exporter=62b4afecdd377fa94310f46467c48a01e61d14db"
echo "oo_platform=linux/arm64"
echo "overlay=none"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export RUSTUP_HOME=/usr/local/rustup
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/a344
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
export PATH="/usr/local/cargo/bin:${PATH}"
mkdir -p /evidence/linux/chain/phases /evidence/linux/chain/steps /cargo-home/bin
cd /src
unset OO_A327_LIVE
printf '%s\n' "no" > /evidence/linux/chain/cargo-update-invoked.txt
printf '%s\n' "a340 already recorded test-lib, decimal_json, absent_row_filter, and clippy at 0 on this commit. This chain does not repeat them." > /evidence/linux/chain/producer-units-not-repeated.txt

record() {
  mkdir -p /evidence/linux/chain/phases
  printf '%s\n' "$2" > "/evidence/linux/chain/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/chain/chain.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/chain/rustc-version.txt
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
  > /evidence/linux/chain/archive-sha256.txt
if ! cmp -s /evidence/linux/chain/archive-sha256.txt /evidence/source/expected.sha256; then
  echo "producer archive hashes do not match 21cc9624" >&2
  record archive-hash 2
fi
if ! grep -q 'fn live_numeric_proof' crates/oo-gateway/src/bin/query-pg.rs; then
  record archive-hash 2
fi
printf '%s\n' none > /evidence/linux/chain/overlay.txt
record archive-hash 0

/cargo-home/bin/sha256 file /export-src/Cargo.lock /export-src/Cargo.toml > /evidence/linux/chain/exporter-lock.txt
if ! grep -q 'cc614d3de29ee73fb29ed7bfa9761102f9a3fe26d27244e8e76000016a791d22' /evidence/linux/chain/exporter-lock.txt; then
  echo "exporter archive lock drifted" >&2
  record exporter-hash 1
fi
record exporter-hash 0

if [ ! -d /cargo-home/a344-clang-root/usr/lib ]; then
  set +e
  bash /extract-libclang.sh > /evidence/linux/chain/libclang-extract.log 2>&1
  extract_exit=$?
  set -e
  record libclang "$extract_exit"
else
  echo "libclang_root=reused" > /evidence/linux/chain/libclang-extract.log
  record libclang 0
fi
if [ ! -s /evidence/linux/chain/libclang-dir.txt ]; then
  find /cargo-home/a344-clang-root/usr/lib \( -name 'libclang.so' -o -name 'libclang-*.so' \) -print -quit | xargs dirname > /evidence/linux/chain/libclang-dir.txt
fi
libdir=$(cat /evidence/linux/chain/libclang-dir.txt)
multiarch=/cargo-home/a344-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a344-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/usr/local/cargo/bin:/cargo-home/a344-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a344-clang-root/usr/share/pkgconfig"
real=$(find /cargo-home/a344-clang-root/usr/lib -name 'libclang-19.so.19' -print -quit)
devso=$(find /cargo-home/a344-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  export LIBCLANG_PATH="$(dirname "$devso")"
fi
export LD_LIBRARY_PATH="${LIBCLANG_PATH}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
if [ -z "$real" ] || [ ! -f "$llvm_inc/stdbool.h" ]; then
  record libclang-load 1
fi
record libclang-load 0

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked > /evidence/linux/chain/fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"

echo "=== cargo build pipeline bins ==="
set +e
cargo build --manifest-path Cargo.toml --locked --offline -p oo-pipeline --bin oo-pipeline --bin init-pg --bin analyze-pg \
  > /evidence/linux/chain/build-pipeline.log 2>&1
pipe_exit=$?
set -e
record build-pipeline "$pipe_exit"

echo "=== cargo build builder bins ==="
set +e
cargo build --manifest-path Cargo.toml --locked --offline -p oo-builder --bin oo-builder --bin reviewed-effective-oos --bin role-pack-bundle \
  > /evidence/linux/chain/build-builder.log 2>&1
builder_exit=$?
set -e
record build-builder "$builder_exit"

echo "=== cargo build gateway bins ==="
set +e
cargo build --manifest-path Cargo.toml --locked --offline -p oo-gateway --bin reviewed-policy --bin install-column-acl --bin provision-mapping-reader --bin query-pg \
  > /evidence/linux/chain/build-gateway.log 2>&1
gateway_exit=$?
set -e
record build-gateway "$gateway_exit"

echo "=== live numeric proof ==="
mkdir -p /target/a327
rm -f /target/a327/a327-role-pack-bundle.json /target/a327/a327-role-pack-bundle.json.role-pack-bundle-staging
# query-pg refuses an audit path that already exists. A retry on the same
# evidence directory must drop the previous receipts before the proof.
rm -f /evidence/linux/chain/audits/*.jsonl \
  /evidence/linux/chain/stop.json \
  /evidence/linux/chain/outcome.json \
  /evidence/linux/chain/outcome.txt
rm -rf /evidence/linux/chain/out/complete \
  /evidence/linux/chain/out/partial \
  /evidence/linux/chain/out/formula
export OO_A327_LIVE=1
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --bin query-pg -- --exact a327_live::live_numeric_proof --nocapture --test-threads=1 \
  > /evidence/linux/chain/test-live.log 2>&1
live_exit=$?
set -e
if [ "$live_exit" -ne 0 ]; then
  record test-live "$live_exit"
fi
if ! grep -q 'test a327_live::live_numeric_proof ... ok' /evidence/linux/chain/test-live.log; then
  echo "live test did not record a pass" >&2
  record test-live 2
fi
if [ ! -s /evidence/linux/chain/steps/amounts.stdout ]; then
  echo "compiled amounts stdout is missing" >&2
  record test-live 2
fi
record test-live 0

echo "=== frozen exporter binary ==="
set +e
cargo build --manifest-path /export-src/Cargo.toml --locked --offline --target-dir /target/export \
  > /evidence/linux/chain/build-exporter.log 2>&1
export_build=$?
set -e
record build-exporter "$export_build"
test -x /target/export/debug/query-result-export

echo "=== partial query-pg max_rows 1 ==="
set +e
rustc --edition 2021 /tools/chain-oracle.rs -o /cargo-home/bin/chain-oracle \
  > /evidence/linux/chain/oracle-build.log 2>&1
oracle_build=$?
set -e
record oracle-build "$oracle_build"
/cargo-home/bin/chain-oracle partial-request \
  /evidence/linux/chain/requests/amounts.json \
  /evidence/linux/chain/requests/partial.json \
  > /evidence/linux/chain/partial-request.txt
record partial-request $?
set +e
/target/a344/debug/query-pg --request /evidence/linux/chain/requests/partial.json \
  > /evidence/linux/chain/steps/partial.stdout \
  2> /evidence/linux/chain/steps/partial.stderr
partial_exit=$?
set -e
printf '%s\n' "$partial_exit" > /evidence/linux/chain/steps/partial.exit
if [ "$partial_exit" -ne 0 ]; then
  record query-partial "$partial_exit"
fi
record query-partial 0

echo "=== exporter complete, partial, formula header ==="
mkdir -p /evidence/linux/chain/out/complete \
  /evidence/linux/chain/out/partial \
  /evidence/linux/chain/out/formula
set +e
/target/export/debug/query-result-export --request /evidence/linux/chain/requests/export-complete.json \
  > /evidence/linux/chain/out/complete.stdout \
  2> /evidence/linux/chain/out/complete.stderr
complete_exit=$?
/target/export/debug/query-result-export --request /evidence/linux/chain/requests/export-partial.json \
  > /evidence/linux/chain/out/partial.stdout \
  2> /evidence/linux/chain/out/partial.stderr
truncated_exit=$?
/target/export/debug/query-result-export --request /evidence/linux/chain/requests/export-formula.json \
  > /evidence/linux/chain/out/formula.stdout \
  2> /evidence/linux/chain/out/formula.stderr
formula_exit=$?
set -e
printf '%s\n' "$complete_exit" > /evidence/linux/chain/out/complete.exit
printf '%s\n' "$truncated_exit" > /evidence/linux/chain/out/partial.exit
printf '%s\n' "$formula_exit" > /evidence/linux/chain/out/formula.exit
if [ "$complete_exit" -ne 0 ] || [ "$formula_exit" -ne 0 ] || [ "$truncated_exit" -ne 1 ]; then
  echo "export exits complete=$complete_exit partial=$truncated_exit formula=$formula_exit" >&2
  record export 1
fi
record export 0

echo "=== hand gold ==="
/cargo-home/bin/chain-oracle check complete \
  /evidence/linux/chain/steps/amounts.stdout \
  /evidence/linux/chain/out/complete/export.csv \
  /evidence/linux/chain/out/complete/export.sidecar.json \
  /evidence/gold/amounts.csv \
  > /evidence/linux/chain/gold-complete.txt
record gold-complete $?
/cargo-home/bin/chain-oracle check partial \
  /evidence/linux/chain/steps/partial.stdout \
  /evidence/linux/chain/out/partial/export.csv \
  /evidence/linux/chain/out/partial/export.sidecar.json \
  /evidence/gold/partial.csv \
  > /evidence/linux/chain/gold-partial.txt
record gold-partial $?
/cargo-home/bin/chain-oracle check formula \
  /evidence/linux/chain/steps/amounts.stdout \
  /evidence/linux/chain/out/formula/export.csv \
  /evidence/linux/chain/out/formula/export.sidecar.json \
  /evidence/gold/formula.csv \
  > /evidence/linux/chain/gold-formula.txt
record gold-formula $?

before=b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474
after=$(/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}')
printf 'before=%s\nafter=%s\n' "$before" "$after" > /evidence/linux/chain/lock-hash.txt
if [ "$before" != "$after" ]; then
  record lock-hash 1
fi
record lock-hash 0
/cargo-home/bin/sha256 file /target/a344/debug/query-pg /target/export/debug/query-result-export \
  > /evidence/linux/chain/bin-sha256.txt || true
printf '%s\n' 0 > /evidence/linux/chain/chain.exit
echo "chain_done"
exit 0
