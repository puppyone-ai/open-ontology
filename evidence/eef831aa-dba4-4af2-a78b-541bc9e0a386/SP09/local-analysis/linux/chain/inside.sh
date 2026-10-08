#!/bin/bash
# Build query-pg from the 21cc9624 archive, run the frozen live numeric proof,
# then export that stdout with the frozen 62b4afec exporter.
# Producer unit tests and producer Clippy are not repeated. Rust 1.94.
# No cargo update. No moving source.
# Origin: a333 actual-pg-chain linux/chain/inside.sh
# SHA-256 292a700bb0a323de575e51738f46158f108c41dd676378b0121319e2631171a9
# Changes: a386 evidence paths and cargo target. The bundle path stays
# /target/a327 because the frozen live test writes that path.
# Tail: bind this run's audit to tools/local-analysis and prove the
# rust_exact CSV, Chinese SVG, manifest, uncited, cancel, and admission
# refusals. DuckDB, Arrow, Parquet, Excel, and a CJK font file stay refused.
set -u
echo "oo_task=sp05-query-result-export-chain-review"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a386"
echo "oo_producer=21cc96241a841b47dbbcb116549030e38a643ba0"
echo "oo_exporter=62b4afecdd377fa94310f46467c48a01e61d14db"
echo "oo_platform=linux/arm64"
echo "overlay=none"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export RUSTUP_HOME=/usr/local/rustup
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/a386
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

if [ ! -d /cargo-home/a386-clang-root/usr/lib ]; then
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
  find /cargo-home/a386-clang-root/usr/lib \( -name 'libclang.so' -o -name 'libclang-*.so' \) -print -quit | xargs dirname > /evidence/linux/chain/libclang-dir.txt
fi
libdir=$(cat /evidence/linux/chain/libclang-dir.txt)
multiarch=/cargo-home/a386-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a386-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/usr/local/cargo/bin:/cargo-home/a386-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a386-clang-root/usr/share/pkgconfig"
real=$(find /cargo-home/a386-clang-root/usr/lib -name 'libclang-19.so.19' -print -quit)
devso=$(find /cargo-home/a386-clang-root/usr/lib -name 'libclang.so' -print -quit)
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
/target/a386/debug/query-pg --request /evidence/linux/chain/requests/partial.json \
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
/cargo-home/bin/sha256 file /target/a386/debug/query-pg /target/export/debug/query-result-export \
  > /evidence/linux/chain/bin-sha256.txt || true

echo "=== local-analysis on this run's export ==="
echo "oo_task_tail=sp09-w05-local-analysis"
rm -rf \
  /evidence/linux/chain/out/analysis \
  /evidence/linux/chain/out/analysis-wide \
  /evidence/linux/chain/out/analysis-partial \
  /evidence/linux/chain/out/analysis-cancel \
  /evidence/linux/chain/out/analysis-parquet \
  /evidence/linux/chain/out/analysis-xlsx \
  /evidence/linux/chain/out/analysis-duckdb
rm -f \
  /evidence/linux/chain/lineage-complete.json \
  /evidence/linux/chain/lineage-partial.json \
  /evidence/linux/chain/out/cancel-mark
set +e
rustc --edition 2021 /evidence/tools/lineage-file.rs -o /cargo-home/bin/lineage-file \
  > /evidence/linux/chain/lineage-file-build.log 2>&1
lineage_build=$?
set -e
record lineage-file-build "$lineage_build"
test -x /cargo-home/bin/lineage-file

if [ -x /target/debug/local-analysis ]; then
  printf '%s\n' /target/debug/local-analysis > /evidence/linux/chain/analysis-bin.txt
else
  set +e
  cargo build --manifest-path /local-analysis/Cargo.toml --locked --offline --target-dir /target/la \
    > /evidence/linux/chain/build-local-analysis.log 2>&1
  la_build=$?
  set -e
  record build-local-analysis "$la_build"
  printf '%s\n' /target/la/debug/local-analysis > /evidence/linux/chain/analysis-bin.txt
fi
LA=$(cat /evidence/linux/chain/analysis-bin.txt)
test -x "$LA"
/cargo-home/bin/sha256 file "$LA" > /evidence/linux/chain/analysis-bin-sha256.txt

build_lineage() {
  digest=$(/cargo-home/bin/sha256 file "$1" | awk '{print $1}')
  set +e
  /cargo-home/bin/lineage-file "$1" "$digest" "$2" > "$3"
  code=$?
  set -e
  printf '%s\n' "$code"
}

complete_lineage=$(build_lineage \
  /evidence/linux/chain/audits/amounts.jsonl \
  /evidence/linux/chain/lineage-complete.json \
  /evidence/linux/chain/lineage-complete.txt)
record lineage-complete "$complete_lineage"
partial_lineage=$(build_lineage \
  /evidence/linux/chain/audits/partial.jsonl \
  /evidence/linux/chain/lineage-partial.json \
  /evidence/linux/chain/lineage-partial.txt)
record lineage-partial "$partial_lineage"

mkdir -p \
  /evidence/linux/chain/out/analysis \
  /evidence/linux/chain/out/analysis-wide \
  /evidence/linux/chain/out/analysis-partial \
  /evidence/linux/chain/out/analysis-cancel \
  /evidence/linux/chain/out/analysis-parquet \
  /evidence/linux/chain/out/analysis-xlsx \
  /evidence/linux/chain/out/analysis-duckdb
printf '%s\n' keep > /evidence/linux/chain/out/analysis-parquet/keep.txt
printf '%s\n' keep > /evidence/linux/chain/out/analysis-xlsx/keep.txt
printf '%s\n' keep > /evidence/linux/chain/out/analysis-duckdb/keep.txt
printf '%s\n' cancel > /evidence/linux/chain/out/cancel-mark

run_la() {
  set +e
  "$LA" --request "$2" \
    > "/evidence/linux/chain/out/${1}.stdout" \
    2> "/evidence/linux/chain/out/${1}.stderr"
  code=$?
  set -e
  printf '%s\n' "$code" > "/evidence/linux/chain/out/${1}.exit"
  echo "$code"
}

amount_exit=$(run_la amount /evidence/linux/chain/requests/analysis-amount.json)
wide_exit=$(run_la wide /evidence/linux/chain/requests/analysis-wide.json)
partial_exit=$(run_la partial-analysis /evidence/linux/chain/requests/analysis-partial.json)
cancel_exit=$(run_la cancel /evidence/linux/chain/requests/analysis-cancel.json)
parquet_exit=$(run_la parquet /evidence/linux/chain/requests/analysis-parquet.json)
xlsx_exit=$(run_la xlsx /evidence/linux/chain/requests/analysis-xlsx.json)
engine_exit=$(run_la duckdb /evidence/linux/chain/requests/analysis-duckdb.json)
printf 'amount=%s wide=%s partial=%s cancel=%s parquet=%s xlsx=%s engine=%s\n' \
  "$amount_exit" "$wide_exit" "$partial_exit" "$cancel_exit" \
  "$parquet_exit" "$xlsx_exit" "$engine_exit" \
  > /evidence/linux/chain/analysis-exits.txt

printf 'entry_id,amount\r\n1,12345678901234.5678\r\n2,-12345678901234.5678\r\n3,0.0001\r\n4,-0.0001\r\n5,0.0000\r\n' \
  > /evidence/linux/chain/expected-amount.csv
printf 'entry_id,wide\r\n1,1234567890123456789012345678901234.5678\r\n2,-1234567890123456789012345678901234.5678\r\n3,0.0001\r\n4,-0.0001\r\n5,0.0000\r\n' \
  > /evidence/linux/chain/expected-wide.csv
printf 'local-analysis: uncited\n' > /evidence/linux/chain/expected-uncited.txt
printf 'local-analysis: canceled\n' > /evidence/linux/chain/expected-canceled.txt
printf 'local-analysis: parquet\n' > /evidence/linux/chain/expected-parquet.txt
printf 'local-analysis: xlsx\n' > /evidence/linux/chain/expected-xlsx.txt
printf 'local-analysis: engine\n' > /evidence/linux/chain/expected-engine.txt

fail_proof() {
  printf '%s\n' "$1" > /evidence/linux/chain/analysis-proof.txt
  echo "analysis proof failed: $1" >&2
  record analysis-proof 1
}

qid=$(sed -n 's/^query_id=//p' /evidence/linux/chain/lineage-complete.txt)
[ -n "$qid" ] || fail_proof missing-query-id
[ "$amount_exit" = "0" ] || fail_proof amount-exit
[ ! -s /evidence/linux/chain/out/amount.stdout ] || fail_proof amount-stdout
[ ! -s /evidence/linux/chain/out/amount.stderr ] || fail_proof amount-stderr
cmp -s /evidence/linux/chain/expected-amount.csv /evidence/linux/chain/out/analysis/analysis.csv || fail_proof amount-csv
svg=/evidence/linux/chain/out/analysis/analysis.svg
man=/evidence/linux/chain/out/analysis/analysis.manifest.json
grep -F '金额' "$svg" >/dev/null || fail_proof svg-title
grep -F '数值已核对' "$svg" >/dev/null || fail_proof svg-citable
if grep -F '不可引用' "$svg" >/dev/null; then fail_proof svg-cited-has-uncited; fi
grep -F 'width="400"' "$svg" >/dev/null || fail_proof svg-width-400
grep -F 'width="1"' "$svg" >/dev/null || fail_proof svg-width-1
grep -F 'width="0"' "$svg" >/dev/null || fail_proof svg-width-0
grep -F 'xmlns="http://www.w3.org/2000/svg"' "$svg" >/dev/null || fail_proof svg-xmlns
if grep -F '<script' "$svg" >/dev/null; then fail_proof svg-script; fi
if grep -F 'foreignObject' "$svg" >/dev/null; then fail_proof svg-foreign; fi
if grep -E 'href=' "$svg" >/dev/null; then fail_proof svg-href; fi
http_only=$(grep -o 'http://[^" ]*' "$svg" | sort -u)
[ "$http_only" = "http://www.w3.org/2000/svg" ] || fail_proof svg-http
grep -F '"closes_sp09_w05":false' "$man" >/dev/null || fail_proof manifest-open
grep -F '"citable":true' "$man" >/dev/null || fail_proof manifest-citable
grep -F '"duckdb":"not_admitted"' "$man" >/dev/null || fail_proof manifest-duckdb
grep -F '"arrow":"not_admitted"' "$man" >/dev/null || fail_proof manifest-arrow
grep -F '"xlsx":"not_admitted"' "$man" >/dev/null || fail_proof manifest-xlsx
grep -F '"parquet":"not_admitted"' "$man" >/dev/null || fail_proof manifest-parquet
grep -F '"cjk_font":"not_bundled"' "$man" >/dev/null || fail_proof manifest-font
grep -F '"engine":"rust_exact"' "$man" >/dev/null || fail_proof manifest-engine
grep -F '"svg":"rust_native"' "$man" >/dev/null || fail_proof manifest-svg
grep -F "\"query_id\":\"${qid}\"" "$man" >/dev/null || fail_proof manifest-qid
grep -F '"null_values":1' "$man" >/dev/null || fail_proof manifest-null-values
grep -F '"null_groups":0' "$man" >/dev/null || fail_proof manifest-null-groups
grep -F '"group_count":5' "$man" >/dev/null || fail_proof manifest-groups
grep -F 'null_value' "$man" >/dev/null || fail_proof manifest-warning
if find /evidence/linux/chain/out/analysis -name '*.partial' | grep -q .; then
  fail_proof residue
fi
if grep -F "'" /evidence/linux/chain/out/analysis/analysis.csv >/dev/null; then
  fail_proof apostrophe
fi

[ "$wide_exit" = "0" ] || fail_proof wide-exit
[ ! -s /evidence/linux/chain/out/wide.stderr ] || fail_proof wide-stderr
cmp -s /evidence/linux/chain/expected-wide.csv /evidence/linux/chain/out/analysis-wide/analysis.csv || fail_proof wide-csv
grep -F '宽金额' /evidence/linux/chain/out/analysis-wide/analysis.svg >/dev/null || fail_proof wide-title
grep -F '1234567890123456789012345678901234.5678' /evidence/linux/chain/out/analysis-wide/analysis.svg >/dev/null || fail_proof wide-svg
grep -F -- '-1234567890123456789012345678901234.5678' /evidence/linux/chain/out/analysis-wide/analysis.svg >/dev/null || fail_proof wide-neg
if grep -F "'" /evidence/linux/chain/out/analysis-wide/analysis.csv >/dev/null; then
  fail_proof wide-apostrophe
fi

[ "$partial_exit" = "1" ] || fail_proof partial-exit
cmp -s /evidence/linux/chain/expected-uncited.txt /evidence/linux/chain/out/partial-analysis.stderr || fail_proof partial-stderr
grep -F '不可引用' /evidence/linux/chain/out/analysis-partial/analysis.svg >/dev/null || fail_proof partial-svg
if grep -F '数值已核对' /evidence/linux/chain/out/analysis-partial/analysis.svg >/dev/null; then
  fail_proof partial-checked
fi
grep -F '"citable":false' /evidence/linux/chain/out/analysis-partial/analysis.manifest.json >/dev/null || fail_proof partial-manifest
grep -F '"closes_sp09_w05":false' /evidence/linux/chain/out/analysis-partial/analysis.manifest.json >/dev/null || fail_proof partial-open

[ "$cancel_exit" = "1" ] || fail_proof cancel-exit
cmp -s /evidence/linux/chain/expected-canceled.txt /evidence/linux/chain/out/cancel.stderr || fail_proof cancel-stderr
if find /evidence/linux/chain/out/analysis-cancel -type f | grep -q .; then
  fail_proof cancel-wrote
fi

[ "$parquet_exit" = "3" ] || fail_proof parquet-exit
cmp -s /evidence/linux/chain/expected-parquet.txt /evidence/linux/chain/out/parquet.stderr || fail_proof parquet-stderr
[ "$xlsx_exit" = "3" ] || fail_proof xlsx-exit
cmp -s /evidence/linux/chain/expected-xlsx.txt /evidence/linux/chain/out/xlsx.stderr || fail_proof xlsx-stderr
[ "$engine_exit" = "3" ] || fail_proof engine-exit
cmp -s /evidence/linux/chain/expected-engine.txt /evidence/linux/chain/out/duckdb.stderr || fail_proof engine-stderr
for refused in analysis-parquet analysis-xlsx analysis-duckdb; do
  if find "/evidence/linux/chain/out/${refused}" -name 'analysis.*' | grep -q .; then
    fail_proof "${refused}-wrote"
  fi
  [ -f "/evidence/linux/chain/out/${refused}/keep.txt" ] || fail_proof "${refused}-keep"
done

printf '%s\n' ok > /evidence/linux/chain/analysis-proof.txt
record analysis-proof 0
printf '%s\n' 0 > /evidence/linux/chain/chain.exit
echo "chain_done"
exit 0
