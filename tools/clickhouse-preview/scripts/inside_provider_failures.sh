#!/bin/bash
# Same customer catalog and model config. Timeout, HTTP 499, and cost_unknown.
# No target database. Empty cache. No pack on a failure.
set -u
mkdir -p /out/raw /work
trap 'status=$?; echo "$status" > /out/raw/inside.exit' EXIT
log() { printf '%s\n' "$*" | tee -a /out/raw/inside.log >&2; }
fail() { log "FAIL $*"; exit 1; }

export HOME=/work
export TMPDIR=/work
export CARGO_HOME=/cargo
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_BUILD_JOBS=1
export CARGO_TARGET_DIR=/target/pg

log "phase rustc $(date -u +%Y-%m-%dT%H:%M:%SZ)"
rustc --version | tee /out/raw/rustc.txt
grep -q '1.94.0' /out/raw/rustc.txt || fail rustc
rustc -vV | tee /out/raw/rustc-vv.txt
grep -q '^release: 1.94.0$' /out/raw/rustc-vv.txt || fail rustc-release
sha256sum /src/Cargo.lock | tee /out/raw/cargo-lock.sha
grep -q '^b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474 ' /out/raw/cargo-lock.sha || fail cargo-lock

build_pg() {
  local name="$1"
  log "phase $name $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  cargo --locked --offline build --manifest-path /src/tools/pg-builder/Cargo.toml --bin pg-builder --jobs 1 > "/out/raw/${name}.log" 2>&1
  local code=$?
  if [ "$code" -ne 0 ]; then
    log "retry $name locked network"
    cargo --locked build --manifest-path /src/tools/pg-builder/Cargo.toml --bin pg-builder --jobs 1 > "/out/raw/${name}.retry.log" 2>&1
    code=$?
    cat "/out/raw/${name}.retry.log" >> "/out/raw/${name}.log"
  fi
  echo "$code" > "/out/raw/${name}.exit"
  return "$code"
}

PG_BIN=/target/pg/debug/pg-builder
NEED_SHA='8cd9306fd68805f7a79929066f58000ec19c4019d61aa7abca5d1f37cf61e13e'
if [ -f "$PG_BIN" ] && sha256sum "$PG_BIN" | tee /out/raw/pg-builder.sha | grep -q "^${NEED_SHA} "; then
  log "phase pg-bin reuse $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo 0 > /out/raw/pg-bin.exit
else
  if ! build_pg pg-bin; then
    if grep -q -e libclang -e bindgen "/out/raw/pg-bin.log" "/out/raw/pg-bin.retry.log" 2>/dev/null; then
      log "phase apt-clang"
      apt-get update > /out/raw/apt-update.log 2>&1
      apt-get install -y --no-install-recommends clang libclang-dev pkg-config > /out/raw/apt-clang.log 2>&1
      echo "$?" > /out/raw/apt-clang.exit
      test "$(cat /out/raw/apt-clang.exit)" = "0" || fail apt-clang
      libclang=$(find /usr -name 'libclang.so*' -print -quit)
      printf '%s\n' "$libclang" > /out/raw/libclang-path.txt
      test -n "$libclang" || fail libclang
      LIBCLANG_PATH=$(dirname "$libclang")
      export LIBCLANG_PATH
      build_pg pg-bin-clang || fail pg-bin-clang
    else
      fail pg-bin
    fi
  fi
  sha256sum "$PG_BIN" | tee /out/raw/pg-builder.sha
  grep -q "^${NEED_SHA} " /out/raw/pg-builder.sha || fail pg-builder-sha
fi

log "phase forwarder-build"
mkdir -p /target/pg/tmp
TMPDIR=/target/pg/tmp rustc --edition 2021 -O -o /target/pg/forwarder /forwarder.rs > /out/raw/forwarder-build.log 2>&1
echo "$?" > /out/raw/forwarder-build.exit
test "$(cat /out/raw/forwarder-build.exit)" = "0" || fail forwarder-build
test -x /target/pg/forwarder || fail forwarder-exec

IDENTITY="ee499c466e9cb518fb15c425b7842014130119b7ba9430cfd83b9fd588515c0c"
DIGEST="a50009f44d9dfa312021d23d6fad1de695ac39ae8e4657b213cad71bf1541799"
INPUT_SHA="255506fdc1679846af7091492b1b5ba562cb0f167bd325725ee82bd2d6c563e5"
OLD_COST_SHA="3ea5146968a1c619047007ece92f20e54156a8010ae7c16ad35a722ca07cb753"

no_secret() {
  local file
  for file in "$@"; do
    test -f "$file" || continue
    if grep -q 'Authorization' "$file"; then
      fail "logged-authorization $file"
    fi
    if grep -q -F -f /run/llm-token "$file"; then
      fail "bearer-logged $file"
    fi
  done
}

require_common() {
  local dir="$1"
  local file="$dir/result.json"
  test -f "$file" || fail "missing-result $dir"
  grep -q "\"identity_hash\":\"$IDENTITY\"" "$file" || fail "identity $dir"
  grep -q "\"input_digest\":\"$DIGEST\"" "$file" || fail "digest $dir"
  grep -q '"pack":"not_produced"' "$file" || fail "pack $dir"
  grep -q '"ready":false' "$file" || fail "ready $dir"
  grep -q '"publication":"not_published"' "$file" || fail "publication $dir"
  grep -q '"cache_store":"not_stored"' "$file" || fail "cache-store $dir"
  grep -q '"model":"called"' "$file" || fail "model $dir"
  grep -q '"source_id":"clickhouse_268"' "$file" || fail "source $dir"
  grep -q '92f6b595153c1e872e4f957730de74ebc51c5fb3b5c0a1fc4043087ce3b7b728' "$file" && fail "old-identity $dir"
  grep -q 'b1c6bb9eca85ed565640b806295eb0b7bfb7a22e8fd30e1e18a6929ccf066b16' "$file" && fail "nine-identity $dir"
  test ! -e "$dir/pack" || fail "half-pack $dir"
  test ! -e "$dir/role-pack" || fail "half-role $dir"
  test ! -e "$dir/policy.json" || fail "half-policy $dir"
  test ! -e "$dir/authority.json" || fail "half-authority $dir"
  cmp -s "$dir/model-input.json" /expected/model-input.json || fail "model-input $dir"
  find "$dir/cache" -type f | tee "/out/raw/$(basename "$dir")-cache-files.txt"
  test ! -s "/out/raw/$(basename "$dir")-cache-files.txt" || fail "cache-file $dir"
}

run_case() {
  local name="$1"
  local config="$2"
  shift 2
  local mode="$1"
  shift
  local dir="/out/$name"
  rm -rf "$dir"
  mkdir -p "$dir/cache"
  local fwd_log="/out/raw/${name}-forwarder.log"
  rm -f "$fwd_log"
  log "phase $name $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  /target/pg/forwarder "$mode" "$fwd_log" "$@" > "/out/raw/${name}-forwarder.stdout" 2> "/out/raw/${name}-forwarder.err" &
  local fwd=$!
  local ready=0
  local _
  for _ in $(seq 1 50); do
    if [ -f "$fwd_log" ] && grep -q FORWARDER_READY "$fwd_log"; then
      ready=1
      break
    fi
    sleep 0.1
  done
  if [ "$ready" != "1" ]; then
    kill "$fwd" 2>/dev/null || true
    wait "$fwd" 2>/dev/null || true
    fail "forwarder-ready $name"
  fi
  set +e
  /target/pg/debug/pg-builder sealed \
    --config "$config" \
    --catalog /plan/catalog.json \
    --settings /plan/settings.json \
    --visibility /plan/visibility.json \
    --cache "$dir/cache" \
    --out "$dir" \
    --reviewer oo-dl01w1-40 \
    --reviewed-at 2026-10-11 \
    --dataset readings.final \
    > "/out/raw/${name}.stdout" 2> "/out/raw/${name}.stderr"
  local code=$?
  set -u
  echo "$code" > "/out/raw/${name}.exit"
  kill "$fwd" 2>/dev/null || true
  wait "$fwd" 2>/dev/null || true
  no_secret "/out/raw/${name}.stdout" "/out/raw/${name}.stderr" "/out/raw/${name}-forwarder.stdout" "/out/raw/${name}-forwarder.err" "$fwd_log" "$dir/result.json" "$dir/model-response.bin"
  printf '%s\n' "$code"
}

log "phase timeout deadline_ms=120000"
timeout_code=$(run_case timeout /plan/builder-config.json timeout)
test "$timeout_code" = "1" || fail timeout-exit
require_common /out/timeout
grep -q '"outcome":"provider_failure"' /out/timeout/result.json || fail timeout-outcome
grep -q '"reason":"timeout"' /out/timeout/result.json || fail timeout-reason
grep -q '"http_status":0' /out/timeout/result.json || fail timeout-http
grep -q "\"input_sha256\":\"$INPUT_SHA\"" /out/timeout/result.json || fail timeout-input-sha
test ! -e /out/timeout/model-response.bin || fail timeout-body

log "phase http-499"
status_code=$(run_case http-499 /plan/builder-config.json status 499 /out/status-499-body.json)
test "$status_code" = "1" || fail http-499-exit
require_common /out/http-499
grep -q '"outcome":"provider_failure"' /out/http-499/result.json || fail http-499-outcome
grep -q '"reason":"http_status"' /out/http-499/result.json || fail http-499-reason
grep -q '"http_status":499' /out/http-499/result.json || fail http-499-status
grep -q "\"input_sha256\":\"$INPUT_SHA\"" /out/http-499/result.json || fail http-499-input-sha

cost_ok() {
  local dir="$1"
  test -f "$dir/result.json" || return 1
  grep -q '"outcome":"cost_unknown"' "$dir/result.json" || return 1
  grep -q '"reason":"unknown_cost"' "$dir/result.json" || return 1
  grep -q "\"identity_hash\":\"$IDENTITY\"" "$dir/result.json" || return 1
  grep -q "\"input_digest\":\"$DIGEST\"" "$dir/result.json" || return 1
  grep -q "\"input_sha256\":\"$INPUT_SHA\"" "$dir/result.json" && return 1
  grep -q "\"input_sha256\":\"$OLD_COST_SHA\"" "$dir/result.json" && return 1
  return 0
}

log "phase cost-unknown-live"
live_code=$(run_case cost-unknown-live /out/builder-config-unknown.json proxy)
printf '%s\n' "$live_code" > /out/raw/cost-unknown-live.exit
if cost_ok /out/cost-unknown-live; then
  require_common /out/cost-unknown-live
  printf '%s\n' live > /out/raw/cost-unknown-source.txt
  log "cost-unknown live"
else
  log "phase cost-unknown validated-content"
  content_code=$(run_case cost-unknown /out/builder-config-unknown.json complete /out/completion-body.json)
  test "$content_code" = "1" || fail cost-unknown-exit
  require_common /out/cost-unknown
  cost_ok /out/cost-unknown || fail cost-unknown-result
  printf '%s\n' validated-content > /out/raw/cost-unknown-source.txt
  log "cost-unknown validated-content"
fi

log "INSIDE_OK $(date -u +%Y-%m-%dT%H:%M:%SZ)"
exit 0
