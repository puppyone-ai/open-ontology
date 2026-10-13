#!/bin/bash
# Linux 1.94 customer five-column chain. Driver source is this task. Other crates are archives.
set -u
mkdir -p /out/raw /work /out/builder /out/builder-cache /out/builder-cache-replay /operator
trap 'status=$?; echo "$status" > /out/raw/inside.exit' EXIT
log() { echo "$*" | tee -a /out/raw/inside.log; }
fail() { log "FAIL $*"; exit 1; }

log "phase rustc $(date -u +%Y-%m-%dT%H:%M:%SZ)"
export HOME=/work
export TMPDIR=/work
export CARGO_HOME=/cargo
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_BUILD_JOBS=1
unset OO_NATIVE_ADMISSION_BIN OO_NATIVE_BINDING_REGISTRY OO_NATIVE_BINDING_REGISTRY_SHA256 || true
rustc --version | tee /out/raw/rustc.txt
grep -q '1.94.0' /out/raw/rustc.txt || fail rustc
rustc -vV | tee /out/raw/rustc-vv.txt
grep -q '^release: 1.94.0$' /out/raw/rustc-vv.txt || fail rustc-release

cargo_locked() {
  local name="$1"
  shift
  log "phase $name $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  cargo --locked --offline "$@" > "/out/raw/${name}.log" 2>&1
  local code=$?
  if [ "$code" -ne 0 ]; then
    log "retry $name locked network"
    cargo --locked "$@" > "/out/raw/${name}.retry.log" 2>&1
    code=$?
    cat "/out/raw/${name}.retry.log" >> "/out/raw/${name}.log"
  fi
  echo "$code" > "/out/raw/${name}.exit"
  if [ "$code" -ne 0 ]; then
    fail "$name"
  fi
}

log "phase driver-msrv $(date -u +%Y-%m-%dT%H:%M:%SZ)"
cd /driver
export CARGO_TARGET_DIR=/target/driver
run_gate() {
  local name="$1"
  shift
  log "phase $name $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  cargo --locked --offline "$@" > "/out/raw/${name}.log" 2>&1
  local code=$?
  if [ "$code" -ne 0 ]; then
    log "retry $name locked network"
    cargo --locked "$@" > "/out/raw/${name}.retry.log" 2>&1
    code=$?
    cat "/out/raw/${name}.retry.log" >> "/out/raw/${name}.log"
  fi
  echo "$code" > "/out/raw/${name}.exit"
  if [ "$code" -ne 0 ]; then
    fail "$name"
  fi
}

run_gate driver-test test --jobs 1 --all-targets
if ! rustup run 1.94.0 cargo clippy --version >/dev/null 2>&1; then
  rustup component add clippy --toolchain 1.94.0 > /out/raw/clippy-component.log 2>&1
  echo "$?" > /out/raw/clippy-component.exit
  test "$(cat /out/raw/clippy-component.exit)" = "0" || fail clippy-component
fi
run_gate driver-clippy clippy --jobs 1 --all-targets -- -D warnings
run_gate driver-build build --jobs 1
sha256sum /target/driver/debug/ch-reader | tee /out/raw/ch-reader.sha
sha256sum /target/driver/debug/ch-decode | tee /out/raw/ch-decode.sha
test -s /out/raw/ch-reader.sha || fail ch-reader-missing
echo READER_BUILT > /out/raw/READER_BUILT
log "READER_BUILT $(date -u +%Y-%m-%dT%H:%M:%SZ)"

log "phase non-utf-decode"
ready=0
for _ in $(seq 1 600); do
  if [ -s /out/raw/non-utf.arrow ]; then
    ready=1
    break
  fi
  sleep 1
done
if [ "$ready" != "1" ]; then
  echo 1 > /out/raw/non-utf-classify.exit
  log "FAIL non-utf-arrow-missing"
else
  /target/driver/debug/ch-decode classify-bytes --ipc /out/raw/non-utf.arrow --column payload --expect-hex ff00fe --out /out/raw/non-utf-classify.json > /out/raw/non-utf-classify.stdout 2> /out/raw/non-utf-classify.stderr
  echo "$?" > /out/raw/non-utf-classify.exit
fi
test "$(cat /out/raw/non-utf-classify.exit)" = "0" || fail non-utf-classify
grep -q '"classification": "binary_preserved"' /out/raw/non-utf-classify.json || fail non-utf-preserved

# Archive crates link pg_query via bindgen. The pinned rust:1.94 image has no libclang.
# Same apt step as product-run-10. The image digest is unchanged; rustc stays 1.94.0.
if ! command -v clang >/dev/null 2>&1; then
  log "phase apt-clang"
  apt-get update > /out/raw/apt-update.log 2>&1
  apt-get install -y --no-install-recommends clang libclang-dev pkg-config > /out/raw/apt-clang.log 2>&1
  echo "$?" > /out/raw/apt-clang.exit
  if [ "$(cat /out/raw/apt-clang.exit)" != "0" ]; then
    fail apt-clang
  fi
fi
libclang=$(find /usr -name 'libclang.so*' -print -quit)
printf '%s\n' "$libclang" > /out/raw/libclang-path.txt
if [ -z "$libclang" ]; then
  fail libclang
fi
export LIBCLANG_PATH=$(dirname "$libclang")
log "LIBCLANG_PATH $LIBCLANG_PATH"

log "phase product-bins $(date -u +%Y-%m-%dT%H:%M:%SZ)"
export CARGO_TARGET_DIR=/target/op
cargo_locked operator-build build --manifest-path /op/tools/operator-preview/Cargo.toml --bin operator-preview
sha256sum /target/op/debug/operator-preview | tee /out/raw/operator.sha
cp /target/op/debug/operator-preview /operator/operator-preview
chmod 755 /operator/operator-preview
export CARGO_TARGET_DIR=/target/ws
cargo_locked pipeline-bin build --manifest-path /src/Cargo.toml -p oo-pipeline --bin oo-pipeline --bin init-pg
export CARGO_TARGET_DIR=/target/pg
cargo_locked pg-bin build --manifest-path /src/tools/pg-builder/Cargo.toml --bin pg-builder
sha256sum /target/ws/debug/oo-pipeline | tee /out/raw/oo-pipeline.sha
sha256sum /target/ws/debug/init-pg | tee /out/raw/init-pg.sha
sha256sum /target/pg/debug/pg-builder | tee /out/raw/pg-builder.sha

log "phase wait-bind $(date -u +%Y-%m-%dT%H:%M:%SZ)"
bound=0
for _ in $(seq 1 1800); do
  if [ -f /out/raw/BIND_READY ]; then
    bound=1
    break
  fi
  if [ -f /out/raw/BIND_FAIL ]; then
    fail bind-fail
  fi
  sleep 1
done
test "$bound" = "1" || fail bind-timeout

sha256sum /admission/oo-source-admission | tee /out/raw/admission-bin.sha
grep -q '^a205eb5797f69d2c95e4e1cc533fce5642be5424df7669fb8a49fef4026eeaf2 ' /out/raw/admission-bin.sha || fail admission-bin
if grep -q 'secret://' /plan/registry.json /plan/manifest-negative.json /plan/manifest-positive.json; then
  fail registry-secret
fi
REG_SHA=$(sha256sum /plan/registry.json | awk '{print $1}')
echo "$REG_SHA" > /out/raw/registry.sha

log "phase init-pg"
init_code=1
for _ in $(seq 1 30); do
  /target/ws/debug/init-pg < /plan/init-pg.json > /out/raw/init-pg.json 2> /out/raw/init-pg.stderr
  init_code=$?
  if [ "$init_code" -eq 0 ]; then
    break
  fi
  sleep 2
done
echo "$init_code" > /out/raw/init-pg.exit
test "$init_code" -eq 0 || fail init-pg

no_secret() {
  local file="$1"
  if [ -f "$file" ] && grep -q 'secret://' "$file"; then
    rm -f "$file"
    fail "secret-in-$file"
  fi
}

run_import() {
  local name="$1"
  local manifest="$2"
  log "phase import-$name"
  env \
    OO_NATIVE_ADMISSION_BIN=/admission/oo-source-admission \
    OO_NATIVE_BINDING_REGISTRY=/plan/registry.json \
    OO_NATIVE_BINDING_REGISTRY_SHA256="$REG_SHA" \
    /target/pg/debug/pg-builder import-native \
      --manifest "$manifest" \
      --out "/out/raw/${name}.json" \
      --pipeline /target/ws/debug/oo-pipeline \
      --stderr "/out/raw/${name}.stderr"
  echo "$?" > "/out/raw/${name}.exit"
  no_secret "/out/raw/${name}.json"
  no_secret "/out/raw/${name}.stderr"
}
run_import negative /plan/manifest-negative.json
test "$(cat /out/raw/negative.exit)" = "1" || fail "negative-exit-$(cat /out/raw/negative.exit)"
grep -q '"load_state":"not_sealed"' /out/raw/negative.json || fail negative-state
grep -q '"run_id":null' /out/raw/negative.json || fail negative-run
run_import positive /plan/manifest-positive.json
test "$(cat /out/raw/positive.exit)" = "0" || fail "positive-exit-$(cat /out/raw/positive.exit)"
grep -q '"load_state":"awaiting_publish"' /out/raw/positive.json || fail positive-state
names=$(grep -o '"logical_name":"[^"]*"' /out/raw/positive.json | head -5 | tr '\n' ' ')
expect='"logical_name":"reading_id" "logical_name":"ts" "logical_name":"amount" "logical_name":"note" "logical_name":"qty" '
test "$names" = "$expect" || fail "fields-$names"
grep -q '"arrow_type":"decimal256","logical_name":"amount","oid":1700,"provenance":"decimal256 stored as numeric; coefficient is not rounded","typmod":4980750' /out/raw/positive.json || fail amount-precision
grep -q '"logical_name":"note","oid":17' /out/raw/positive.json || fail note-bytea
grep -q '"logical_name":"ts"' /out/raw/positive.json || fail ts-column
grep -q '"row_count":3' /out/raw/positive.json || fail row-count

log "phase import-seal-customer"
/target/pg/debug/pg-builder import-seal --catalog /plan/catalog.json --out /out/raw/seal-customer.json > /out/raw/seal-customer.stdout 2> /out/raw/seal-customer.stderr
echo "$?" > /out/raw/seal-customer.exit
test "$(cat /out/raw/seal-customer.exit)" = "0" || fail seal-customer
grep -q '"outcome":"draft_compiled"' /out/raw/seal-customer.json || fail seal-customer-outcome
grep -q '"model":"not_called"' /out/raw/seal-customer.json || fail seal-customer-model

token_bytes=$(wc -c < /run/llm-token | awk '{print $1}')
test "$token_bytes" = "6" || fail token-file

log "phase forwarder"
mkdir -p /operator
rustc --edition 2021 -O -o /out/raw/forwarder /forwarder.rs > /out/raw/forwarder-build.log 2>&1
echo "$?" > /out/raw/forwarder-build.exit
test "$(cat /out/raw/forwarder-build.exit)" = "0" || fail forwarder-build
/out/raw/forwarder > /out/raw/forwarder.stdout 2> /out/raw/forwarder.err &
fwd=$!
ready=0
for _ in $(seq 1 50); do
  if [ -f /out/raw/forwarder.log ] && grep -q FORWARDER_READY /out/raw/forwarder.log; then
    ready=1
    break
  fi
  sleep 0.1
done
test "$ready" = "1" || fail forwarder-ready

seal_build() {
  local name="$1"
  local out_dir="$2"
  log "phase sealed-$name $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  /target/pg/debug/pg-builder sealed \
    --config /plan/builder-config.json \
    --catalog /plan/catalog.json \
    --settings /plan/settings.json \
    --visibility /plan/visibility.json \
    --cache /out/builder-cache \
    --out "$out_dir" \
    --reviewer oo-dl01w1-40 \
    --reviewed-at 2026-10-11 \
    --dataset readings.final \
    --target-host oo-dl01w1-40-pg \
    --target-port 5432 \
    --target-database previewdb \
    --target-user oo_preview_login \
    --login oo_preview_login \
    --target-ca /run/pg-secrets/pg-ca.crt \
    --target-password /run/pg-secrets/login_password \
    > "/out/raw/builder-${name}.stdout" 2> "/out/raw/builder-${name}.stderr"
  echo "$?" > "/out/raw/builder-${name}.exit"
}

seal_build fresh /out/builder
kill "$fwd" 2>/dev/null || true
wait "$fwd" 2>/dev/null || true
rm -f /out/raw/forwarder
test "$(cat /out/raw/builder-fresh.exit)" = "0" || fail builder-fresh
grep -q '"outcome":"packed"' /out/builder/result.json || fail outcome-not-packed
grep -q '"model":"called"' /out/builder/result.json || fail model-not-called
grep -q '"cache":"miss"' /out/builder/result.json || fail cache-not-miss
grep -q '"cache_store":"stored"' /out/builder/result.json || fail cache-not-stored
grep -q '"pack":"produced"' /out/builder/result.json || fail pack-not-produced
grep -q '"source_id":"clickhouse_268"' /out/builder/result.json || fail builder-source
grep -q 'b1c6bb9eca85ed565640b806295eb0b7bfb7a22e8fd30e1e18a6929ccf066b16' /out/builder/result.json && fail nine-column-identity
grep -q '182324dbd09f19804f9898b4997e0be186478c20d43bdd1824a72dfafdb334db' /out/builder/result.json && fail nine-column-pack
test "$(tr -d '[:space:]' < /out/raw/upstream.status)" = "200" || fail http-status
if grep -q 'Authorization' /out/raw/forwarder.log /out/raw/builder-fresh.stdout /out/raw/builder-fresh.stderr; then
  fail logged-authorization
fi
cp /out/builder/result.json /out/raw/builder-fresh-result.json
fresh_id=$(sed -n 's/.*"identity_hash":"\([a-f0-9][a-f0-9]*\)".*/\1/p' /out/builder/result.json | head -1)
printf '%s\n' "$fresh_id" > /out/raw/identity-hash.txt
test "${#fresh_id}" -eq 64 || fail identity-length

log "phase sealed-cache"
seal_build cache /out/builder-cache-replay
test "$(cat /out/raw/builder-cache.exit)" = "0" || fail builder-cache
grep -q '"outcome":"packed"' /out/builder-cache-replay/result.json || fail cache-outcome
grep -q '"cache":"hit"' /out/builder-cache-replay/result.json || fail cache-not-hit
grep -q '"model":"cache_hit"' /out/builder-cache-replay/result.json || fail cache-model
cache_id=$(sed -n 's/.*"identity_hash":"\([a-f0-9][a-f0-9]*\)".*/\1/p' /out/builder-cache-replay/result.json | head -1)
test "$fresh_id" = "$cache_id" || fail cache-identity
echo SEALED_OK > /out/raw/SEALED_OK
log "SEALED_OK $(date -u +%Y-%m-%dT%H:%M:%SZ)"

log "phase wait-provision"
provisioned=0
for _ in $(seq 1 180); do
  if [ -f /out/raw/PROVISION_OK ]; then
    provisioned=1
    break
  fi
  if [ -f /out/raw/PROVISION_FAIL ]; then
    fail provision
  fi
  sleep 1
done
test "$provisioned" = "1" || fail provision-timeout
test -f /out/raw/AUTHORITIES_OK || fail authorities

run_op() {
  local name="$1"
  shift
  /operator/operator-preview "$@" > "/out/raw/${name}.stdout" 2> "/out/raw/${name}.stderr"
  echo "$?" > "/out/raw/${name}.exit"
  no_secret "/out/raw/${name}.stdout"
  no_secret "/out/raw/${name}.stderr"
}
fresh_audit() {
  local dir="/out/raw/audit/$1"
  rm -rf "$dir"
  mkdir -p "$dir"
  chmod 700 "$dir"
}
problems=""
note_fail() { problems="${problems} $1"; log "FAIL $1"; }

fresh_audit describe
run_op describe describe --authority /out/raw/authorities/describe.json
test "$(cat /out/raw/describe.exit)" = "0" || note_fail describe
grep -q '"outcome": "described"' /out/raw/describe.stdout || note_fail describe-outcome
fresh_audit guard
run_op query-denied query --authority /out/raw/authorities/guard.json --query /out/queries/query-denied.json
test "$(cat /out/raw/query-denied.exit)" = "6" || note_fail "guard-exit-$(cat /out/raw/query-denied.exit)"
grep -q '"class": "guard"' /out/raw/query-denied.stdout || note_fail guard-class
fresh_audit widen
run_op query-widen query --authority /out/raw/authorities/widen.json --query /out/queries/query-denied.json
test "$(cat /out/raw/query-widen.exit)" = "9" || note_fail "widen-exit-$(cat /out/raw/query-widen.exit)"
fresh_audit query
run_op query query --authority /out/raw/authorities/query.json --query /out/queries/query.json
test "$(cat /out/raw/query.exit)" = "0" || note_fail typed-query
grep -q '"outcome": "succeeded"' /out/raw/query.stdout || note_fail typed-outcome

log "phase inflight-cancel"
table=$(grep -o '"physical_name":"pt_[0-9a-f]\{60\}"' /out/raw/positive.json | head -1 | sed 's/.*:"//;s/"$//')
printf '%s\n' "$table" > /out/raw/NEED_LOCK
locked=0
for _ in $(seq 1 60); do
  if [ -f /out/raw/LOCK_HELD ]; then
    locked=1
    break
  fi
  sleep 0.2
done
if [ "$locked" != "1" ]; then
  note_fail lock-held
else
  fresh_audit cancel
  /operator/operator-preview query --authority /out/raw/authorities/cancel.json --query /out/queries/query-cancel.json > /out/raw/query-cancel.stdout 2> /out/raw/query-cancel.stderr &
  qpid=$!
  running=""
  for _ in $(seq 1 200); do
    for state_file in /out/builder/state/executions/*.json; do
      if [ -f "$state_file" ] && grep -q '"status":"running"' "$state_file"; then
        running=$(basename "$state_file" .json)
        break
      fi
    done
    if [ -n "$running" ]; then
      break
    fi
    sleep 0.05
  done
  if [ -z "$running" ]; then
    status=0
    wait "$qpid" || status=$?
    echo "$status" > /out/raw/query-cancel.exit
    note_fail cancel-target-not-running
  else
    printf '%s\n' "$running" > /out/raw/RUNNING
    seen=0
    for _ in $(seq 1 200); do
      if [ -f /out/raw/BACKEND_SEEN ]; then
        seen=1
        break
      fi
      sleep 0.05
    done
    if [ "$seen" != "1" ]; then
      note_fail backend-not-seen
    else
      run_op cancel-inflight cancel --authority /out/raw/authorities/cancel.json --execution "$running"
    fi
    status=0
    wait "$qpid" || status=$?
    echo "$status" > /out/raw/query-cancel.exit
  fi
  printf '%s\n' RELEASE_LOCK > /out/raw/RELEASE_LOCK
  test "$(cat /out/raw/cancel-inflight.exit 2>/dev/null || true)" = "0" || note_fail cancel-inflight
  grep -q '"outcome": "sent"' /out/raw/cancel-inflight.stdout || note_fail cancel-sent
  test "$(cat /out/raw/query-cancel.exit 2>/dev/null || true)" = "0" || note_fail canceled-query
  grep -q '"outcome": "canceled"' /out/raw/query-cancel.stdout || note_fail canceled-outcome
  grep -q '"sqlstate": "57014"' /out/raw/query-cancel.stdout || note_fail canceled-sqlstate
  grep -q '"rollback_confirmed": true' /out/raw/query-cancel.stdout || note_fail canceled-rollback
fi
for _ in $(seq 1 150); do
  if [ -f /out/raw/LOCK_RELEASED ]; then
    break
  fi
  sleep 0.2
done

printf '%s\n' stop > /out/raw/NEED_SOURCE_OFF
off=0
for _ in $(seq 1 120); do
  if [ -f /out/raw/SOURCE_STOPPED ]; then
    off=1
    break
  fi
  sleep 1
done
if [ "$off" != "1" ]; then
  note_fail source-off
else
  fresh_audit after-off
  run_op query-after-off query --authority /out/raw/authorities/after-off.json --query /out/queries/query.json
  test "$(cat /out/raw/query-after-off.exit)" = "0" || note_fail after-off
  grep -q '"outcome": "succeeded"' /out/raw/query-after-off.stdout || note_fail after-off-outcome
  fresh_audit reopen
  run_op query-reopen query --authority /out/raw/authorities/reopen.json --query /out/queries/query.json
  test "$(cat /out/raw/query-reopen.exit)" = "0" || note_fail reopen
  grep -q '"outcome": "succeeded"' /out/raw/query-reopen.stdout || note_fail reopen-outcome
fi
test "$(cat /out/raw/non-utf-classify.exit 2>/dev/null || echo 1)" = "0" || note_fail non-utf-classify
grep -q '"classification": "binary_preserved"' /out/raw/non-utf-classify.json 2>/dev/null || note_fail non-utf-preserved
oracle_ok=0
for _ in $(seq 1 120); do
  if [ -f /out/raw/customer-verdict.txt ]; then
    oracle_ok=1
    break
  fi
  sleep 1
done
if [ "$oracle_ok" != "1" ] || ! grep -q '^MATCH$' /out/raw/customer-verdict.txt; then
  note_fail oracle
fi
if [ -n "$problems" ]; then
  log "PROBLEMS$problems"
  printf '%s\n' "$problems" > /out/raw/PROBLEMS
  exit 1
fi
exit 0
