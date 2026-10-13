#!/bin/bash
# Build pg-builder from the mounted archive and post one invalid bearer.
set -eu
mkdir -p /out/raw /work /out/failure-cache /out/builder-401
trap 'status=$?; echo "$status" > /out/raw/inside.exit' EXIT

log() { echo "$*" | tee -a /out/raw/inside.log; }
fail() { log "FAIL $*"; exit 1; }

log "phase smoke-rustc $(date -u +%Y-%m-%dT%H:%M:%SZ)"
rustc --version | tee /out/raw/rustc.txt
grep -q '1.94.0' /out/raw/rustc.txt || fail rustc

export HOME=/work
export TMPDIR=/work
export CARGO_HOME=/cargo
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_BUILD_JOBS=1
unset OO_NATIVE_ADMISSION_BIN OO_NATIVE_BINDING_REGISTRY OO_NATIVE_BINDING_REGISTRY_SHA256 || true

token_bytes=$(wc -c < /run/invalid-bearer | awk '{print $1}')
if [ "$token_bytes" -lt 8 ] || [ "$token_bytes" -gt 257 ]; then
  fail token-file
fi

cd /src
cargo_locked() {
  local name="$1"
  shift
  log "phase $name $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  set +e
  cargo --locked --offline "$@" > "/out/raw/${name}.log" 2>&1
  local code=$?
  if [ "$code" -ne 0 ]; then
    log "retry $name locked network"
    cargo --locked "$@" > "/out/raw/${name}.retry.log" 2>&1
    code=$?
    cat "/out/raw/${name}.retry.log" >> "/out/raw/${name}.log"
  fi
  set -e
  echo "$code" > "/out/raw/${name}.exit"
  if [ "$code" -ne 0 ]; then
    fail "$name"
  fi
}

export CARGO_TARGET_DIR=/target/pg
cargo_locked pg-bin build --manifest-path /src/tools/pg-builder/Cargo.toml --bin pg-builder
sha256sum /target/pg/debug/pg-builder | tee /out/raw/pg-builder.sha
sha256sum /src/Cargo.lock | tee /out/raw/cargo-lock.sha
log "PG_BUILDER_OK $(date -u +%Y-%m-%dT%H:%M:%SZ)"

seal_one() {
  local name="$1"
  local catalog="$2"
  log "phase import-seal $name $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  set +e
  /target/pg/debug/pg-builder import-seal \
    --catalog "$catalog" \
    --out "/out/raw/seal-${name}.json" \
    > "/out/raw/seal-${name}.stdout" 2> "/out/raw/seal-${name}.stderr"
  echo "$?" > "/out/raw/seal-${name}.exit"
  set -e
}

seal_one customer /plan/catalog.json
seal_one datetime64 /inputs/catalog-datetime64.json
seal_one array /inputs/catalog-array.json
test "$(cat /out/raw/seal-customer.exit)" = "0" || fail seal-customer
grep -q '"outcome":"draft_compiled"' /out/raw/seal-customer.json || fail seal-customer-outcome
test "$(cat /out/raw/seal-datetime64.exit)" = "1" || fail seal-datetime64
grep -q 'type.ambiguous' /out/raw/seal-datetime64.json || fail seal-datetime64-code
test "$(cat /out/raw/seal-array.exit)" = "1" || fail seal-array
grep -q 'type.ambiguous' /out/raw/seal-array.json || fail seal-array-code

log "phase forwarder $(date -u +%Y-%m-%dT%H:%M:%SZ)"
rustc --edition 2021 -O -o /target/pg/forwarder /forwarder.rs > /out/raw/forwarder-build.log 2>&1
echo "$?" > /out/raw/forwarder-build.exit
test "$(cat /out/raw/forwarder-build.exit)" = "0" || fail forwarder-build
/target/pg/forwarder > /out/raw/forwarder.stdout 2> /out/raw/forwarder.err &
fwd=$!
ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
  if [ -f /out/raw/forwarder.log ] && grep -q FORWARDER_READY /out/raw/forwarder.log; then
    ready=1
    break
  fi
  sleep 0.1
done
test "$ready" = "1" || fail forwarder-ready

log "phase sealed-401 $(date -u +%Y-%m-%dT%H:%M:%SZ)"
dataset="${OO_DATASET:?dataset missing}"
set +e
/target/pg/debug/pg-builder sealed \
  --config /plan/builder-config.json \
  --catalog /plan/catalog.json \
  --settings /plan/settings.json \
  --visibility /plan/visibility.json \
  --cache /out/failure-cache \
  --out /out/builder-401 \
  --reviewer oo-dl01w1-40 \
  --reviewed-at 2026-10-11 \
  --dataset "$dataset" \
  > /out/raw/builder.stdout 2> /out/raw/builder.stderr
echo "$?" > /out/raw/builder.exit
set -e
kill "$fwd" 2>/dev/null || true
wait "$fwd" 2>/dev/null || true
rm -f /target/pg/forwarder

problems=""
add() { problems="${problems} $1"; }
result=/out/builder-401/result.json
model_input=/out/builder-401/model-input.json
test -f "$result" || fail result-missing
test -f "$model_input" || fail model-input-missing
grep -q '"outcome":"provider_failure"' "$result" || add outcome
grep -q '"reason":"http_status"' "$result" || add reason
grep -q '"http_status":401' "$result" || add http_status
grep -q '"model":"called"' "$result" || add model
grep -q '"pack":"not_produced"' "$result" || add pack
grep -q '"cache_store":"not_stored"' "$result" || add cache_store
grep -q '"ready":false' "$result" || add ready
grep -q '"publication":"not_published"' "$result" || add publication
grep -q '"socket":"opened"' "$result" || add socket
grep -q '"source_id":"clickhouse_268"' "$result" || add source_id
grep -q '"source_engine":"clickhouse"' "$result" || add source_engine
test "$(cat /out/raw/builder.exit)" = "1" || add builder_exit
cache_files=$(find /out/failure-cache -type f | wc -l | awk '{print $1}')
test "$cache_files" = "0" || add cache_stored
for name in role-pack pack authority.json policy.json effective.json reviewed.json; do
  if [ -e "/out/builder-401/$name" ]; then
    add half_ready
  fi
done
test -f /out/raw/upstream.status || add upstream_missing
test "$(cat /out/raw/upstream.status)" = "401" || add upstream_status
grep -q '"column":"ts"' "$model_input" || add ts_column
grep -q '"source_type":"timestamp_nanos_utc"' "$model_input" || add ts_source_type
grep -q '"target_oid":1700' "$model_input" || add ts_oid
if grep -q '"target_type":"timestamptz"' "$model_input"; then
  add timestamptz
fi
printf '%s\n' "${problems# }" > /out/raw/http401-problems.txt
test -z "$problems" || fail http401-summary
if grep -q 'Authorization' /out/raw/forwarder.log /out/raw/builder.stdout /out/raw/builder.stderr; then
  fail logged-authorization
fi
echo HTTP401_OK > /out/raw/HTTP401_OK
log "HTTP401_OK $(date -u +%Y-%m-%dT%H:%M:%SZ)"
exit 0
