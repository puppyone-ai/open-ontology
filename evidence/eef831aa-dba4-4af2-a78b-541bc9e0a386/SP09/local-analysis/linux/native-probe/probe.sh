#!/bin/bash
# Probe official DuckDB CLI 1.5.6 and Noto Sans CJK SC Sans2.004.
# Writes hashes and SQL trials. Does not compile the product crate.
set -u
log() { printf '%s\n' "$*"; }
fail() { log "PROBE_FAIL $*"; exit 1; }
log "start $(date -u +%Y-%m-%dT%H:%M:%SZ)"
mkdir -p /native /work /evidence/linux/native-probe
if ! command -v curl >/dev/null 2>&1 || ! command -v unzip >/dev/null 2>&1; then
  apt-get update || fail "apt-update"
  apt-get install -y --no-install-recommends curl ca-certificates unzip || fail "apt-install"
fi
cd /work
curl -fsSL -o duck.tgz "https://github.com/duckdb/duckdb/releases/download/v1.5.6/duckdb_cli-linux-arm64.gz" || fail "duck-download"
echo "1af0541e649a3ae34eb20eec2ca564308bf0e8993e5d6dba6be77bfb289a2718  duck.tgz" | sha256sum -c - || fail "duck-archive-hash"
gzip -dc duck.tgz > /native/duckdb || fail "duck-unpack"
chmod 755 /native/duckdb
sha256sum /native/duckdb | tee /evidence/linux/native-probe/duckdb-unpacked.sha256
wc -c /native/duckdb | tee /evidence/linux/native-probe/duckdb-unpacked.bytes
env -i HOME=/tmp /native/duckdb -version > /evidence/linux/native-probe/duckdb-version.txt 2>&1 || fail "duck-version"
log "duckdb version: $(cat /evidence/linux/native-probe/duckdb-version.txt)"

curl -fsSL -o font.zip "https://github.com/notofonts/noto-cjk/releases/download/Sans2.004/08_NotoSansCJKsc.zip" || fail "font-download"
sha256sum font.zip | tee /evidence/linux/native-probe/font-zip.sha256
wc -c font.zip | tee /evidence/linux/native-probe/font-zip.bytes
unzip -l font.zip > /evidence/linux/native-probe/font-list.txt || fail "font-list"
unzip -jo font.zip "*NotoSansCJKsc-Regular.otf" -d /native || fail "font-unzip"
sha256sum /native/NotoSansCJKsc-Regular.otf | tee /evidence/linux/native-probe/font-otf.sha256
wc -c /native/NotoSansCJKsc-Regular.otf | tee /evidence/linux/native-probe/font-otf.bytes
rm -f font.zip duck.tgz

mkdir -p /work/p
cd /work/p
env -i HOME=/tmp /native/duckdb -bail -batch -c "COPY (SELECT '甲' AS group_key, 0::INTEGER AS ordinal, 123456789012345678::DECIMAL(38,4) AS coefficient UNION ALL SELECT '乙', 1, -5::DECIMAL(38,4)) TO 'in.parquet' (FORMAT PARQUET);" > /evidence/linux/native-probe/make-parquet.out 2> /evidence/linux/native-probe/make-parquet.err || fail "make-parquet"
ls -l in.parquet | tee /evidence/linux/native-probe/parquet-ls.txt
ABS=$(pwd)

try_sql() {
  name="$1"
  sql="$2"
  log "===== ${name} ====="
  set +e
  env -i HOME=/tmp /native/duckdb -bail -batch -json -c "$sql" > "/evidence/linux/native-probe/${name}.out" 2> "/evidence/linux/native-probe/${name}.err"
  code=$?
  set -e
  printf '%s\n' "$code" > "/evidence/linux/native-probe/${name}.exit"
  log "exit ${code}"
  log "stdout $(wc -c < "/evidence/linux/native-probe/${name}.out") stderr $(wc -c < "/evidence/linux/native-probe/${name}.err")"
}

Q="SELECT group_key, CAST(SUM(coefficient) AS VARCHAR) AS amount FROM read_parquet('${ABS}/in.parquet') GROUP BY group_key ORDER BY MIN(ordinal);"
try_sql base "$Q"
try_sql extfalse "SET enable_external_access=false; ${Q}"
try_sql allowed "SET threads=1; SET memory_limit='256MB'; SET autoinstall_known_extensions=false; SET autoload_known_extensions=false; SET allowed_directories=['${ABS}']; ${Q}"
try_sql both "SET threads=1; SET memory_limit='256MB'; SET autoinstall_known_extensions=false; SET autoload_known_extensions=false; SET allowed_directories=['${ABS}']; SET enable_external_access=false; ${Q}"
try_sql locked "SET threads=1; SET memory_limit='256MB'; SET autoinstall_known_extensions=false; SET autoload_known_extensions=false; SET allowed_directories=['${ABS}']; SET enable_external_access=false; SET lock_configuration=true; ${Q}"

log "===== slow ====="
start=$(date +%s)
set +e
env -i HOME=/tmp /native/duckdb -bail -batch -c "SELECT count(*) FROM range(20000) a, range(20000) b;" > /evidence/linux/native-probe/slow.out 2> /evidence/linux/native-probe/slow.err
slow_code=$?
set -e
end=$(date +%s)
printf 'exit %s elapsed %s\n' "$slow_code" "$((end - start))" | tee /evidence/linux/native-probe/slow.meta
log "PROBE_OK"
