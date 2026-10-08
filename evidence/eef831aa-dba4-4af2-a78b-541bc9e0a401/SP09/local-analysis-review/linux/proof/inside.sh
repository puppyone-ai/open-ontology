#!/bin/bash
# Independent SP09-W05 review. Immutable archives only.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a386/SP09/local-analysis/linux/chain-full/inside.sh
# sha256 10ca54506f3d6437b79216d4f415d1d11e0f0962d748a84b213dba325638406d
# Producer 21cc96241a841b47dbbcb116549030e38a643ba0.
# Exporter 62b4afecdd377fa94310f46467c48a01e61d14db.
# local-analysis e2732e323915a332a85f008c605b50c1f4d5e948.
# Changes: owner a401, proof paths, pinned CLI and font fetch, formula header,
# the focused boundary test, and a locked fetch of this package before its
# offline build. The bundle path stays /target/a327.
# Producer unit tests, producer Clippy, and the author whole test matrix are
# not repeated.
set -u
echo "oo_task=sp09-w05-local-analysis-review"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a401"
echo "oo_producer=21cc96241a841b47dbbcb116549030e38a643ba0"
echo "oo_exporter=62b4afecdd377fa94310f46467c48a01e61d14db"
echo "oo_platform=linux/arm64"
echo "overlay=tests/a401_bounds.rs,examples/extract_otf.rs"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export RUSTUP_HOME=/usr/local/rustup
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/a401
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
export PATH="/usr/local/cargo/bin:${PATH}"
mkdir -p /evidence/linux/proof/phases /evidence/linux/proof/steps /cargo-home/bin
cd /src
unset OO_A327_LIVE
printf '%s\n' "no" > /evidence/linux/proof/cargo-update-invoked.txt
printf '%s\n' "Producer units and the author whole matrix are not repeated. This run is the private PostgreSQL export plus the focused boundary test." > /evidence/linux/proof/producer-units-not-repeated.txt

record() {
  mkdir -p /evidence/linux/proof/phases
  printf '%s\n' "$2" > "/evidence/linux/proof/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/proof/chain.exit
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
  echo "producer archive hashes do not match 21cc9624" >&2
  record archive-hash 2
fi
if ! grep -q 'fn live_numeric_proof' crates/oo-gateway/src/bin/query-pg.rs; then
  record archive-hash 2
fi
printf '%s\n' 'tests/a401_bounds.rs examples/extract_otf.rs' > /evidence/linux/proof/overlay.txt
record archive-hash 0

/cargo-home/bin/sha256 file /export-src/Cargo.lock /export-src/Cargo.toml > /evidence/linux/proof/exporter-lock.txt
if ! grep -q 'cc614d3de29ee73fb29ed7bfa9761102f9a3fe26d27244e8e76000016a791d22' /evidence/linux/proof/exporter-lock.txt; then
  echo "exporter archive lock drifted" >&2
  record exporter-hash 1
fi
record exporter-hash 0

if [ ! -d /cargo-home/a401-clang-root/usr/lib ]; then
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
  find /cargo-home/a401-clang-root/usr/lib \( -name 'libclang.so' -o -name 'libclang-*.so' \) -print -quit | xargs dirname > /evidence/linux/proof/libclang-dir.txt
fi
libdir=$(cat /evidence/linux/proof/libclang-dir.txt)
multiarch=/cargo-home/a401-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a401-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/usr/local/cargo/bin:/cargo-home/a401-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a401-clang-root/usr/share/pkgconfig"
real=$(find /cargo-home/a401-clang-root/usr/lib -name 'libclang-19.so.19' -print -quit)
devso=$(find /cargo-home/a401-clang-root/usr/lib -name 'libclang.so' -print -quit)
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
cargo fetch --manifest-path Cargo.toml --locked > /evidence/linux/proof/fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"

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
mkdir -p /target/a327
rm -f /target/a327/a327-role-pack-bundle.json /target/a327/a327-role-pack-bundle.json.role-pack-bundle-staging
# query-pg refuses an audit path that already exists. A retry on the same
# evidence directory must drop the previous receipts before the proof.
rm -f /evidence/linux/proof/audits/*.jsonl \
  /evidence/linux/proof/stop.json \
  /evidence/linux/proof/outcome.json \
  /evidence/linux/proof/outcome.txt
rm -rf /evidence/linux/proof/out/complete \
  /evidence/linux/proof/out/partial \
  /evidence/linux/proof/out/formula
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
if [ ! -s /evidence/linux/proof/steps/amounts.stdout ]; then
  echo "compiled amounts stdout is missing" >&2
  record test-live 2
fi
record test-live 0

echo "=== frozen exporter binary ==="
set +e
cargo build --manifest-path /export-src/Cargo.toml --locked --offline --target-dir /target/export \
  > /evidence/linux/proof/build-exporter.log 2>&1
export_build=$?
set -e
record build-exporter "$export_build"
test -x /target/export/debug/query-result-export

echo "=== partial query-pg max_rows 1 ==="
set +e
rustc --edition 2021 /tools/chain-oracle.rs -o /cargo-home/bin/chain-oracle \
  > /evidence/linux/proof/oracle-build.log 2>&1
oracle_build=$?
set -e
record oracle-build "$oracle_build"
/cargo-home/bin/chain-oracle partial-request \
  /evidence/linux/proof/requests/amounts.json \
  /evidence/linux/proof/requests/partial.json \
  > /evidence/linux/proof/partial-request.txt
record partial-request $?
set +e
/target/a401/debug/query-pg --request /evidence/linux/proof/requests/partial.json \
  > /evidence/linux/proof/steps/partial.stdout \
  2> /evidence/linux/proof/steps/partial.stderr
partial_exit=$?
set -e
printf '%s\n' "$partial_exit" > /evidence/linux/proof/steps/partial.exit
if [ "$partial_exit" -ne 0 ]; then
  record query-partial "$partial_exit"
fi
record query-partial 0

echo "=== export requests ==="
mkdir -p /evidence/linux/proof/requests
cat > /evidence/linux/proof/requests/export-complete.json <<'EOF'
{"version":"oo.query-result-export/v1","source_file":"/evidence/linux/proof/steps/amounts.stdout","output_dir":"/evidence/linux/proof/out/complete","columns":["entry_id","amount","wide"]}
EOF
cat > /evidence/linux/proof/requests/export-partial.json <<'EOF'
{"version":"oo.query-result-export/v1","source_file":"/evidence/linux/proof/steps/partial.stdout","output_dir":"/evidence/linux/proof/out/partial","columns":["entry_id","amount","wide"]}
EOF
cat > /evidence/linux/proof/requests/export-formula.json <<'EOF'
{"version":"oo.query-result-export/v1","source_file":"/evidence/linux/proof/steps/amounts.stdout","output_dir":"/evidence/linux/proof/out/formula","columns":["entry_id","amount","-wide"]}
EOF
record export-requests 0

echo "=== exporter complete, partial, formula header ==="
mkdir -p /evidence/linux/proof/out/complete \
  /evidence/linux/proof/out/partial \
  /evidence/linux/proof/out/formula
set +e
/target/export/debug/query-result-export --request /evidence/linux/proof/requests/export-complete.json \
  > /evidence/linux/proof/out/complete.stdout \
  2> /evidence/linux/proof/out/complete.stderr
complete_exit=$?
/target/export/debug/query-result-export --request /evidence/linux/proof/requests/export-partial.json \
  > /evidence/linux/proof/out/partial.stdout \
  2> /evidence/linux/proof/out/partial.stderr
truncated_exit=$?
/target/export/debug/query-result-export --request /evidence/linux/proof/requests/export-formula.json \
  > /evidence/linux/proof/out/formula.stdout \
  2> /evidence/linux/proof/out/formula.stderr
formula_exit=$?
set -e
printf '%s\n' "$complete_exit" > /evidence/linux/proof/out/complete.exit
printf '%s\n' "$truncated_exit" > /evidence/linux/proof/out/partial.exit
printf '%s\n' "$formula_exit" > /evidence/linux/proof/out/formula.exit
if [ "$complete_exit" -ne 0 ] || [ "$formula_exit" -ne 0 ] || [ "$truncated_exit" -ne 1 ]; then
  echo "export exits complete=$complete_exit partial=$truncated_exit formula=$formula_exit" >&2
  record export 1
fi
record export 0

echo "=== hand gold ==="
/cargo-home/bin/chain-oracle check complete \
  /evidence/linux/proof/steps/amounts.stdout \
  /evidence/linux/proof/out/complete/export.csv \
  /evidence/linux/proof/out/complete/export.sidecar.json \
  /evidence/gold/amounts.csv \
  > /evidence/linux/proof/gold-complete.txt
record gold-complete $?
/cargo-home/bin/chain-oracle check partial \
  /evidence/linux/proof/steps/partial.stdout \
  /evidence/linux/proof/out/partial/export.csv \
  /evidence/linux/proof/out/partial/export.sidecar.json \
  /evidence/gold/partial.csv \
  > /evidence/linux/proof/gold-partial.txt
record gold-partial $?
/cargo-home/bin/chain-oracle check formula \
  /evidence/linux/proof/steps/amounts.stdout \
  /evidence/linux/proof/out/formula/export.csv \
  /evidence/linux/proof/out/formula/export.sidecar.json \
  /evidence/gold/formula.csv \
  > /evidence/linux/proof/gold-formula.txt
record gold-formula $?

before=b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474
after=$(/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}')
printf 'before=%s\nafter=%s\n' "$before" "$after" > /evidence/linux/proof/lock-hash.txt
if [ "$before" != "$after" ]; then
  record lock-hash 1
fi
record lock-hash 0
/cargo-home/bin/sha256 file /target/a401/debug/query-pg /target/export/debug/query-result-export \
  > /evidence/linux/proof/bin-sha256.txt || true

echo "=== local-analysis on this run's export ==="
echo "oo_task_tail=sp09-w05-local-analysis"
rm -rf \
  /evidence/linux/proof/out/analysis \
  /evidence/linux/proof/out/analysis-wide \
  /evidence/linux/proof/out/analysis-partial \
  /evidence/linux/proof/out/analysis-formula \
  /evidence/linux/proof/out/analysis-cancel \
  /evidence/linux/proof/out/analysis-duckdb
rm -f \
  /evidence/linux/proof/lineage-complete.json \
  /evidence/linux/proof/lineage-partial.json \
  /evidence/linux/proof/out/cancel-mark
set +e
rustc --edition 2021 /evidence/tools/lineage-file.rs -o /cargo-home/bin/lineage-file \
  > /evidence/linux/proof/lineage-file-build.log 2>&1
lineage_build=$?
set -e
record lineage-file-build "$lineage_build"
test -x /cargo-home/bin/lineage-file

echo "=== cargo fetch local-analysis --locked ==="
set +e
cargo fetch --manifest-path /local-analysis/Cargo.toml --locked \
  > /evidence/linux/proof/fetch-local-analysis.log 2>&1
la_fetch=$?
set -e
record fetch-local-analysis "$la_fetch"

if [ -x /target/la/debug/local-analysis ]; then
  printf '%s\n' /target/la/debug/local-analysis > /evidence/linux/proof/analysis-bin.txt
else
  set +e
  cargo build --manifest-path /local-analysis/Cargo.toml --locked --offline --target-dir /target/la \
    > /evidence/linux/proof/build-local-analysis.log 2>&1
  la_build=$?
  set -e
  record build-local-analysis "$la_build"
  printf '%s\n' /target/la/debug/local-analysis > /evidence/linux/proof/analysis-bin.txt
fi
LA=$(cat /evidence/linux/proof/analysis-bin.txt)
test -x "$LA"
/cargo-home/bin/sha256 file "$LA" > /evidence/linux/proof/analysis-bin-sha256.txt
echo "=== pinned native cli and font ==="
mkdir -p /native /tmp/native-src
if [ ! -x /native/duckdb ]; then
  curl -fsSL -o /tmp/native-src/duck.tgz "https://github.com/duckdb/duckdb/releases/download/v1.5.6/duckdb_cli-linux-arm64.gz" || record native-fetch 1
  echo "1af0541e649a3ae34eb20eec2ca564308bf0e8993e5d6dba6be77bfb289a2718  /tmp/native-src/duck.tgz" | sha256sum -c - || record native-fetch 1
  gzip -dc /tmp/native-src/duck.tgz > /native/duckdb || record native-fetch 1
  chmod 755 /native/duckdb
fi
sha256sum /native/duckdb | tee /evidence/linux/proof/duckdb-unpacked.sha256
duck_sha=$(awk '{print $1}' /evidence/linux/proof/duckdb-unpacked.sha256)
if [ "$duck_sha" != "c0ac0b79e243ee312b3af29c34ac0b94e518aa44c4345036d5d96c1c042c9d48" ]; then
  record native-fetch 1
fi
env -i HOME=/tmp /native/duckdb -version > /evidence/linux/proof/duckdb-version.txt 2>&1 || record native-fetch 1
if [ ! -f /native/NotoSansCJKsc-Regular.otf ]; then
  curl -fsSL -o /tmp/native-src/font.zip "https://github.com/notofonts/noto-cjk/releases/download/Sans2.004/08_NotoSansCJKsc.zip" || record native-fetch 1
  echo "a927e56f53bd6c3b920bc139c0b94aa36c7d9ad0cf009b159437a1a003581140  /tmp/native-src/font.zip" | sha256sum -c - || record native-fetch 1
  set +e
  cargo build --manifest-path /local-analysis/Cargo.toml --locked --offline --example extract_otf --target-dir /target/la \
    > /evidence/linux/proof/build-extract-otf.log 2>&1
  extract_build=$?
  set -e
  record build-extract-otf "$extract_build"
  set +e
  /target/la/debug/examples/extract_otf /tmp/native-src/font.zip /native/NotoSansCJKsc-Regular.otf NotoSansCJKsc-Regular.otf \
    > /evidence/linux/proof/font-extract.txt 2> /evidence/linux/proof/font-extract.err
  extract_code=$?
  set -e
  record font-extract "$extract_code"
  rm -f /tmp/native-src/font.zip
fi
sha256sum /native/NotoSansCJKsc-Regular.otf | tee /evidence/linux/proof/font-otf.sha256
font_pin=$(awk '{print $1}' /evidence/linux/proof/font-otf.sha256)
if [ "$font_pin" != "2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b" ]; then
  record native-fetch 1
fi
rm -f /tmp/native-src/duck.tgz
grep -F 'v1.5.6' /evidence/linux/proof/duckdb-version.txt >/dev/null || record native-fetch 1
record native-fetch 0

test -x /native/duckdb
test -f /native/NotoSansCJKsc-Regular.otf
set +e
cargo build --manifest-path /local-analysis/Cargo.toml --locked --offline --example xlsx_scan --target-dir /target/la \
  > /evidence/linux/proof/build-xlsx-scan.log 2>&1
scan_build=$?
set -e
record build-xlsx-scan "$scan_build"
test -x /target/la/debug/examples/xlsx_scan

build_lineage() {
  digest=$(/cargo-home/bin/sha256 file "$1" | awk '{print $1}')
  set +e
  /cargo-home/bin/lineage-file "$1" "$digest" "$2" > "$3"
  code=$?
  set -e
  printf '%s\n' "$code"
}

complete_lineage=$(build_lineage \
  /evidence/linux/proof/audits/amounts.jsonl \
  /evidence/linux/proof/lineage-complete.json \
  /evidence/linux/proof/lineage-complete.txt)
record lineage-complete "$complete_lineage"
partial_lineage=$(build_lineage \
  /evidence/linux/proof/audits/partial.jsonl \
  /evidence/linux/proof/lineage-partial.json \
  /evidence/linux/proof/lineage-partial.txt)
record lineage-partial "$partial_lineage"

mkdir -p /evidence/linux/proof/requests
cat > /evidence/linux/proof/requests/analysis-amount.json <<'EOF'
{"version":"oo.local-analysis/v1","export_dir":"/evidence/linux/proof/out/complete","lineage_file":"/evidence/linux/proof/lineage-complete.json","output_dir":"/evidence/linux/proof/out/analysis","operation":"sum","group_column":0,"value_column":1,"group_header":"entry_id","value_header":"amount","chart_title":"金额","cancel_file":null,"engine":"duckdb","xlsx":true,"parquet":true,"duckdb_cli":"/native/duckdb","font_file":"/native/NotoSansCJKsc-Regular.otf"}
EOF
cat > /evidence/linux/proof/requests/analysis-wide.json <<'EOF'
{"version":"oo.local-analysis/v1","export_dir":"/evidence/linux/proof/out/complete","lineage_file":"/evidence/linux/proof/lineage-complete.json","output_dir":"/evidence/linux/proof/out/analysis-wide","operation":"sum","group_column":0,"value_column":2,"group_header":"entry_id","value_header":"wide","chart_title":"宽金额","cancel_file":null,"engine":"rust_exact","xlsx":false,"parquet":true}
EOF
cat > /evidence/linux/proof/requests/analysis-partial.json <<'EOF'
{"version":"oo.local-analysis/v1","export_dir":"/evidence/linux/proof/out/partial","lineage_file":"/evidence/linux/proof/lineage-partial.json","output_dir":"/evidence/linux/proof/out/analysis-partial","operation":"sum","group_column":0,"value_column":1,"group_header":"entry_id","value_header":"amount","chart_title":"金额","cancel_file":null,"engine":"rust_exact","xlsx":false,"parquet":false}
EOF
cat > /evidence/linux/proof/requests/analysis-formula.json <<'EOF'
{"version":"oo.local-analysis/v1","export_dir":"/evidence/linux/proof/out/formula","lineage_file":"/evidence/linux/proof/lineage-complete.json","output_dir":"/evidence/linux/proof/out/analysis-formula","operation":"sum","group_column":0,"value_column":2,"group_header":"entry_id","value_header":"-wide","chart_title":"公式","cancel_file":null,"engine":"rust_exact","xlsx":true,"parquet":false}
EOF
cat > /evidence/linux/proof/requests/analysis-cancel.json <<'EOF'
{"version":"oo.local-analysis/v1","export_dir":"/evidence/linux/proof/out/complete","lineage_file":"/evidence/linux/proof/lineage-complete.json","output_dir":"/evidence/linux/proof/out/analysis-cancel","operation":"sum","group_column":0,"value_column":1,"group_header":"entry_id","value_header":"amount","chart_title":"金额","cancel_file":"/evidence/linux/proof/out/cancel-mark","engine":"rust_exact","xlsx":false,"parquet":false}
EOF
cat > /evidence/linux/proof/requests/analysis-duckdb.json <<'EOF'
{"version":"oo.local-analysis/v1","export_dir":"/evidence/linux/proof/out/complete","lineage_file":"/evidence/linux/proof/lineage-complete.json","output_dir":"/evidence/linux/proof/out/analysis-duckdb","operation":"sum","group_column":0,"value_column":1,"group_header":"entry_id","value_header":"amount","chart_title":"金额","cancel_file":null,"engine":"duckdb","xlsx":false,"parquet":false}
EOF
record analysis-requests 0

mkdir -p \
  /evidence/linux/proof/out/analysis \
  /evidence/linux/proof/out/analysis-wide \
  /evidence/linux/proof/out/analysis-partial \
  /evidence/linux/proof/out/analysis-formula \
  /evidence/linux/proof/out/analysis-cancel \
  /evidence/linux/proof/out/analysis-duckdb
printf '%s\n' keep > /evidence/linux/proof/out/analysis-duckdb/keep.txt
printf '%s\n' cancel > /evidence/linux/proof/out/cancel-mark

run_la() {
  set +e
  "$LA" --request "$2" \
    > "/evidence/linux/proof/out/${1}.stdout" \
    2> "/evidence/linux/proof/out/${1}.stderr"
  code=$?
  set -e
  printf '%s\n' "$code" > "/evidence/linux/proof/out/${1}.exit"
  echo "$code"
}

amount_exit=$(run_la amount /evidence/linux/proof/requests/analysis-amount.json)
wide_exit=$(run_la wide /evidence/linux/proof/requests/analysis-wide.json)
partial_exit=$(run_la partial-analysis /evidence/linux/proof/requests/analysis-partial.json)
cancel_exit=$(run_la cancel /evidence/linux/proof/requests/analysis-cancel.json)
engine_exit=$(run_la duckdb /evidence/linux/proof/requests/analysis-duckdb.json)
formula_analysis_exit=$(run_la formula-analysis /evidence/linux/proof/requests/analysis-formula.json)
printf 'amount=%s wide=%s partial=%s cancel=%s engine=%s formula=%s\n' \
  "$amount_exit" "$wide_exit" "$partial_exit" "$cancel_exit" "$engine_exit" "$formula_analysis_exit" \
  > /evidence/linux/proof/analysis-exits.txt

printf 'entry_id,amount\r\n1,12345678901234.5678\r\n2,-12345678901234.5678\r\n3,0.0001\r\n4,-0.0001\r\n5,0.0000\r\n' \
  > /evidence/linux/proof/expected-amount.csv
printf 'entry_id,wide\r\n1,1234567890123456789012345678901234.5678\r\n2,-1234567890123456789012345678901234.5678\r\n3,0.0001\r\n4,-0.0001\r\n5,0.0000\r\n' \
  > /evidence/linux/proof/expected-wide.csv
printf 'local-analysis: uncited\n' > /evidence/linux/proof/expected-uncited.txt
printf 'local-analysis: canceled\n' > /evidence/linux/proof/expected-canceled.txt
printf 'local-analysis: engine\n' > /evidence/linux/proof/expected-engine.txt

fail_proof() {
  printf '%s\n' "$1" > /evidence/linux/proof/analysis-proof.txt
  echo "analysis proof failed: $1" >&2
  record analysis-proof 1
}

qid=$(sed -n 's/^query_id=//p' /evidence/linux/proof/lineage-complete.txt)
[ -n "$qid" ] || fail_proof missing-query-id
[ "$amount_exit" = "0" ] || fail_proof amount-exit
[ ! -s /evidence/linux/proof/out/amount.stdout ] || fail_proof amount-stdout
[ ! -s /evidence/linux/proof/out/amount.stderr ] || fail_proof amount-stderr
cmp -s /evidence/linux/proof/expected-amount.csv /evidence/linux/proof/out/analysis/analysis.csv || fail_proof amount-csv
svg=/evidence/linux/proof/out/analysis/analysis.svg
man=/evidence/linux/proof/out/analysis/analysis.manifest.json
grep -F '金额' "$svg" >/dev/null || fail_proof svg-title
grep -F '数值已核对' "$svg" >/dev/null || fail_proof svg-citable
grep -F 'DuckDB合计与精确十进制一致' "$svg" >/dev/null || fail_proof svg-duckdb
grep -F '字体已校验' "$svg" >/dev/null || fail_proof svg-font
grep -F "NotoSansCJKsc-Regular.otf" "$svg" >/dev/null || fail_proof svg-font-file
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
test -f /evidence/linux/proof/out/analysis/analysis.parquet || fail_proof parquet-missing
test -f /evidence/linux/proof/out/analysis/analysis.arrow || fail_proof arrow-missing
test -f /evidence/linux/proof/out/analysis/analysis.xlsx || fail_proof xlsx-missing
test -f /evidence/linux/proof/out/analysis/NotoSansCJKsc-Regular.otf || fail_proof font-missing
head -c 4 /evidence/linux/proof/out/analysis/analysis.parquet | cmp -s - <(printf 'PAR1') || fail_proof parquet-magic
grep -a -F 'ARROW1' /evidence/linux/proof/out/analysis/analysis.arrow >/dev/null || fail_proof arrow-magic
head -c 2 /evidence/linux/proof/out/analysis/analysis.xlsx | cmp -s - <(printf 'PK') || fail_proof xlsx-magic
set +e
/target/la/debug/examples/xlsx_scan \
  /evidence/linux/proof/out/analysis/analysis.xlsx \
  12345678901234.5678 \
  > /evidence/linux/proof/xlsx-scan.txt \
  2> /evidence/linux/proof/xlsx-scan.err
scan=$?
set -e
printf '%s\n' "$scan" > /evidence/linux/proof/xlsx-scan.exit
[ "$scan" = "0" ] || fail_proof xlsx-scan
[ ! -s /evidence/linux/proof/xlsx-scan.err ] || fail_proof xlsx-scan-stderr
font_sha=$(/cargo-home/bin/sha256 file /evidence/linux/proof/out/analysis/NotoSansCJKsc-Regular.otf | awk '{print $1}')
[ "$font_sha" = "2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b" ] || fail_proof font-sha
grep -F '"closes_sp09_w05":false' "$man" >/dev/null || fail_proof manifest-open
grep -F '"audited":false' "$man" >/dev/null || fail_proof manifest-audited
grep -F '"citable":true' "$man" >/dev/null || fail_proof manifest-citable
grep -F '"engine":"duckdb"' "$man" >/dev/null || fail_proof manifest-engine
grep -F '"duckdb":"pinned_unaudited"' "$man" >/dev/null || fail_proof manifest-duckdb
grep -F '"arrow":"pinned_unaudited"' "$man" >/dev/null || fail_proof manifest-arrow
grep -F '"parquet":"pinned_unaudited"' "$man" >/dev/null || fail_proof manifest-parquet
grep -F '"xlsx":"pinned_unaudited"' "$man" >/dev/null || fail_proof manifest-xlsx
grep -F '"cjk_font":"pinned_unaudited"' "$man" >/dev/null || fail_proof manifest-font
grep -F '"svg":"rust_native"' "$man" >/dev/null || fail_proof manifest-svg
grep -F "\"query_id\":\"${qid}\"" "$man" >/dev/null || fail_proof manifest-qid
grep -F '"null_values":1' "$man" >/dev/null || fail_proof manifest-null-values
grep -F '"null_groups":0' "$man" >/dev/null || fail_proof manifest-null-groups
grep -F '"group_count":5' "$man" >/dev/null || fail_proof manifest-groups
grep -F 'null_value' "$man" >/dev/null || fail_proof manifest-warning
grep -F 'admission_not_audited' "$man" >/dev/null || fail_proof manifest-admission
grep -F '"duckdb_binary_sha256":"c0ac0b79e243ee312b3af29c34ac0b94e518aa44c4345036d5d96c1c042c9d48"' "$man" >/dev/null || fail_proof manifest-cli
if grep -F 'not_admitted' "$man" >/dev/null; then fail_proof manifest-admitted-word; fi
if find /evidence/linux/proof/out/analysis -name '*.partial' | grep -q .; then
  fail_proof residue
fi
if grep -F "'" /evidence/linux/proof/out/analysis/analysis.csv >/dev/null; then
  fail_proof apostrophe
fi

[ "$wide_exit" = "0" ] || fail_proof wide-exit
[ ! -s /evidence/linux/proof/out/wide.stderr ] || fail_proof wide-stderr
cmp -s /evidence/linux/proof/expected-wide.csv /evidence/linux/proof/out/analysis-wide/analysis.csv || fail_proof wide-csv
grep -F '宽金额' /evidence/linux/proof/out/analysis-wide/analysis.svg >/dev/null || fail_proof wide-title
grep -F '1234567890123456789012345678901234.5678' /evidence/linux/proof/out/analysis-wide/analysis.svg >/dev/null || fail_proof wide-svg
grep -F -- '-1234567890123456789012345678901234.5678' /evidence/linux/proof/out/analysis-wide/analysis.svg >/dev/null || fail_proof wide-neg
grep -F '"engine":"rust_exact"' /evidence/linux/proof/out/analysis-wide/analysis.manifest.json >/dev/null || fail_proof wide-engine
grep -F '"duckdb":"not_invoked"' /evidence/linux/proof/out/analysis-wide/analysis.manifest.json >/dev/null || fail_proof wide-duckdb
grep -F '"audited":false' /evidence/linux/proof/out/analysis-wide/analysis.manifest.json >/dev/null || fail_proof wide-audited
if grep -F "'" /evidence/linux/proof/out/analysis-wide/analysis.csv >/dev/null; then
  fail_proof wide-apostrophe
fi

[ "$partial_exit" = "1" ] || fail_proof partial-exit
cmp -s /evidence/linux/proof/expected-uncited.txt /evidence/linux/proof/out/partial-analysis.stderr || fail_proof partial-stderr
grep -F '不可引用' /evidence/linux/proof/out/analysis-partial/analysis.svg >/dev/null || fail_proof partial-svg
if grep -F '数值已核对' /evidence/linux/proof/out/analysis-partial/analysis.svg >/dev/null; then
  fail_proof partial-checked
fi
grep -F '"citable":false' /evidence/linux/proof/out/analysis-partial/analysis.manifest.json >/dev/null || fail_proof partial-manifest
grep -F '"closes_sp09_w05":false' /evidence/linux/proof/out/analysis-partial/analysis.manifest.json >/dev/null || fail_proof partial-open
grep -F '"audited":false' /evidence/linux/proof/out/analysis-partial/analysis.manifest.json >/dev/null || fail_proof partial-audited

[ "$cancel_exit" = "1" ] || fail_proof cancel-exit
cmp -s /evidence/linux/proof/expected-canceled.txt /evidence/linux/proof/out/cancel.stderr || fail_proof cancel-stderr
if find /evidence/linux/proof/out/analysis-cancel -type f | grep -q .; then
  fail_proof cancel-wrote
fi

[ "$engine_exit" = "3" ] || fail_proof engine-exit
cmp -s /evidence/linux/proof/expected-engine.txt /evidence/linux/proof/out/duckdb.stderr || fail_proof engine-stderr
if find /evidence/linux/proof/out/analysis-duckdb -name 'analysis.*' | grep -q .; then
  fail_proof duckdb-wrote
fi
[ -f /evidence/linux/proof/out/analysis-duckdb/keep.txt ] || fail_proof duckdb-keep

[ "$formula_analysis_exit" = "0" ] || fail_proof formula-exit
[ ! -s /evidence/linux/proof/out/formula-analysis.stderr ] || fail_proof formula-stderr
printf 'entry_id,'"'-wide"'\r\n1,1234567890123456789012345678901234.5678\r\n2,-1234567890123456789012345678901234.5678\r\n3,0.0001\r\n4,-0.0001\r\n5,0.0000\r\n' \
  > /evidence/linux/proof/expected-formula.csv
cmp -s /evidence/linux/proof/expected-formula.csv /evidence/linux/proof/out/analysis-formula/analysis.csv || fail_proof formula-csv
if grep -F "'-1234567890123456789012345678901234.5678" /evidence/linux/proof/out/analysis-formula/analysis.csv >/dev/null; then
  fail_proof formula-typed-minus
fi
grep -F '"audited":false' /evidence/linux/proof/out/analysis-formula/analysis.manifest.json >/dev/null || fail_proof formula-audited
grep -F '"citable":true' /evidence/linux/proof/out/analysis-formula/analysis.manifest.json >/dev/null || fail_proof formula-citable
grep -F '"null_values":1' /evidence/linux/proof/out/analysis-formula/analysis.manifest.json >/dev/null || fail_proof formula-null
set +e
/target/la/debug/examples/xlsx_scan \
  /evidence/linux/proof/out/analysis-formula/analysis.xlsx \
  "'-wide" \
  > /evidence/linux/proof/xlsx-formula.txt \
  2> /evidence/linux/proof/xlsx-formula.err
formula_scan=$?
set -e
printf '%s\n' "$formula_scan" > /evidence/linux/proof/xlsx-formula.exit
[ "$formula_scan" = "0" ] || fail_proof formula-xlsx

printf '%s\n' ok > /evidence/linux/proof/analysis-proof.txt
record analysis-proof 0
echo "=== independent focused bounds ==="
set +e
OO_A401_DUCKDB=/native/duckdb \
OO_A401_FONT=/native/NotoSansCJKsc-Regular.otf \
OO_A401_COMPLETE=/evidence/linux/proof/out/complete \
OO_A401_LINEAGE=/evidence/linux/proof/lineage-complete.json \
OO_A401_PARTIAL=/evidence/linux/proof/out/partial \
OO_A401_PARTIAL_LINEAGE=/evidence/linux/proof/lineage-partial.json \
OO_A401_FORMULA=/evidence/linux/proof/out/formula \
cargo test --manifest-path /local-analysis/Cargo.toml --locked --offline --test a401_bounds --target-dir /target/la --jobs 1 -- --test-threads=1 \
  > /evidence/linux/proof/focused-test.log 2>&1
focused_code=$?
set -e
printf '%s\n' "$focused_code" > /evidence/linux/proof/focused-test.exit
record focused-test "$focused_code"

rustup component add clippy > /evidence/linux/proof/clippy-add.log 2>&1
set +e
cargo clippy --manifest-path /local-analysis/Cargo.toml --locked --offline --all-targets --target-dir /target/la --jobs 1 -- -D warnings \
  > /evidence/linux/proof/clippy.log 2>&1
clippy_code=$?
set -e
printf '%s\n' "$clippy_code" > /evidence/linux/proof/clippy.exit
record clippy "$clippy_code"
printf '%s\n' 0 > /evidence/linux/proof/chain.exit
echo "chain_done"
exit 0
