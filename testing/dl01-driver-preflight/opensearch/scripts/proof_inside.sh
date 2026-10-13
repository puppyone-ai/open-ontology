#!/bin/bash
# Live customer ledger. Precheck refuses before any HTTP. Secrets stay in /private.
set -u
BIN=/bins
PRIV=/private
FIX=/src/fixtures
OUT=/evidence/proof
mkdir -p "$OUT"
INDEX=$(sed -n 's/^  "approved_index": "\([^"]*\)".*/\1/p' "$FIX/customer-request.json" | head -n 1)
WORK=$(sed -n 's/^  "approved_index": "\([^"]*\)".*/\1/p' "$FIX/cancel-workload.json" | head -n 1)
DELETE_ID=$(grep -o '"_id": "[^"]*"' "$FIX/customer-documents.json" | tail -n 1 | sed 's/^"_id": "//; s/"$//')
if [ -z "$INDEX" ] || [ -z "$WORK" ] || [ -z "$DELETE_ID" ]; then
  printf 'customer\n' >"$OUT/stopped"
  exit 2
fi
if [ "$INDEX" = "oo-dl01w1-44-synthetic" ] || [ "$WORK" = "$INDEX" ]; then
  printf 'customer-index\n' >"$OUT/stopped"
  exit 2
fi
printf '%s\n' "$INDEX" >"$OUT/customer-index"
printf '%s\n' "$WORK" >"$OUT/workload-index"
printf '%s\n' "$DELETE_ID" >"$OUT/reconcile-id"

run() {
  name="$1"
  shift
  mkdir -p "$OUT/$name"
  set +e
  "$@" >"$OUT/$name/stdout.txt" 2>"$OUT/$name/stderr.txt"
  code=$?
  set -e
  printf '%s\n' "$code" >"$OUT/$name/exit"
  echo "$name $code"
}

require_exit() {
  name="$1"
  expect="$2"
  got=$(cat "$OUT/$name/exit")
  if [ "$got" != "$expect" ]; then
    printf '%s %s\n' "$name" "$got" >"$OUT/stopped"
    exit 2
  fi
}

refusal_has_no_http() {
  name="$1"
  if ! grep -q '"http_calls": 0' "$OUT/$name/refusal.json"; then
    printf '%s http\n' "$name" >"$OUT/stopped"
    exit 2
  fi
  if ! grep -q '"refused": true' "$OUT/$name/refusal.json"; then
    printf '%s refused\n' "$name" >"$OUT/stopped"
    exit 2
  fi
}

set -e
run precheck-index "$BIN/os-reader" precheck \
  --customer "$FIX/customer-request.json" \
  --object-index oo-dl01w1-44-synthetic \
  --object-field doc_key \
  --out "$OUT/precheck-index"
require_exit precheck-index 4
refusal_has_no_http precheck-index
run precheck-field "$BIN/os-reader" precheck \
  --customer "$FIX/customer-request.json" \
  --object-index "$INDEX" \
  --object-field raw_admin \
  --out "$OUT/precheck-field"
require_exit precheck-field 4
refusal_has_no_http precheck-field
run precheck-unknown "$BIN/os-reader" precheck \
  --customer "$FIX/customer-request.json" \
  --object-index "$INDEX" \
  --object-field not_a_customer_field \
  --out "$OUT/precheck-unknown"
require_exit precheck-unknown 4
refusal_has_no_http precheck-unknown

run setup "$BIN/os-reader" setup \
  --private "$PRIV" --index "$INDEX" --out "$OUT/setup" \
  --expect-version 3.9.0 --mapping "$FIX/mapping.json" --fixture "$FIX/customer-documents.json"
require_exit setup 0
run observe "$BIN/os-reader" observe \
  --private "$PRIV" --index "$INDEX" --out "$OUT/observe" \
  --customer "$FIX/customer-request.json" \
  --source-id oo_dl01w1_44 --secret-ref secret://oo-dl01w1-44/opensearch
require_exit observe 0
if [ ! -x "$BIN/opensearch-preview" ] || [ ! -f "$OUT/observe/observation.json" ]; then
  printf 'decide-missing\n' >"$OUT/stopped"
  exit 2
fi
digest="$(sha256sum "$BIN/os-reader" | awk '{print $1}')"
printf '%s\n' "$digest" >"$OUT/decide.driver-digest"
run decide "$BIN/opensearch-preview" decide \
  --request "$OUT/observe/observation.json" \
  --driver-digest "$digest" \
  --out "$OUT/decide"
require_exit decide 0

mkdir -p "$OUT/read"
"$BIN/os-reader" read --private "$PRIV" --index "$INDEX" --out "$OUT/read" \
  --page-size 3 --keep-pit yes --hold-ms 20000 \
  >"$OUT/read/stdout.txt" 2>"$OUT/read/stderr.txt" &
read_pid=$!
ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
  if [ -f "$OUT/read/ready.flag" ]; then
    ready=1
    break
  fi
  sleep 1
done
printf '%s\n' "$ready" >"$OUT/read/ready.flag.seen"
if [ "$ready" = "1" ]; then
  run stats "$BIN/os-reader" dba-stats --private "$PRIV" --out "$OUT/stats/http.json"
fi
set +e
wait "$read_pid"
read_code=$?
set -e
printf '%s\n' "$read_code" >"$OUT/read/exit"
echo "read $read_code"
if [ "$read_code" != "0" ]; then
  printf 'read %s\n' "$read_code" >"$OUT/stopped"
  exit 2
fi
run visibility "$BIN/os-reader" visibility --private "$PRIV" --index "$INDEX" --out "$OUT/visibility" --pit-file "$OUT/read/pit.id"
run reconcile "$BIN/os-reader" reconcile --private "$PRIV" --index "$INDEX" --out "$OUT/reconcile" --delete-id "$DELETE_ID"
run negative "$BIN/os-reader" negative --private "$PRIV" --index "$INDEX" --out "$OUT/negative"
run shape "$BIN/os-reader" shape --private "$PRIV" --out "$OUT/shape" --document "$FIX/shape-document.json"
run cancel "$BIN/os-reader" cancel --private "$PRIV" --index "$WORK" --out "$OUT/cancel" --workload "$FIX/cancel-workload.json"
run cancel-late "$BIN/os-reader" cancel-late --private "$PRIV" --index "$WORK" --out "$OUT/cancel-late" --workload "$FIX/cancel-workload.json"
run deadline "$BIN/os-reader" deadline --private "$PRIV" --index "$WORK" --out "$OUT/deadline" --timeout-ms 20000 --workload "$FIX/cancel-workload.json"
run rowcap "$BIN/os-reader" read --private "$PRIV" --index "$INDEX" --out "$OUT/rowcap" --page-size 3 --row-cap 2
run bytecap "$BIN/os-reader" read --private "$PRIV" --index "$INDEX" --out "$OUT/bytecap" --page-size 3 --byte-cap 256
run block-cast "$BIN/os-reader" block-cast --out "$OUT/block-cast/blocked.json"
run server-audit "$BIN/os-reader" server-audit --private "$PRIV" --out "$OUT/server-audit"
if [ -f "$OUT/read/batch.arrow" ]; then
  run decode "$BIN/os-decode" --ipc "$OUT/read/batch.arrow" --out "$OUT/decode/decoded.json"
fi
if [ -f "$OUT/decode/decoded.json" ]; then
  run judge "$BIN/os-judge" \
    --fixture "$FIX/customer-documents.json" --mapping "$FIX/mapping.json" \
    --decoded "$OUT/decode/decoded.json" --read-dir "$OUT/read" \
    --visibility "$OUT/visibility/visibility.json" \
    --reconcile "$OUT/reconcile/reconcile.json" \
    --negative "$OUT/negative/negative.json" \
    --shape "$OUT/shape/shape.json" \
    --cancel "$OUT/cancel/cancel.json" \
    --cancel-late "$OUT/cancel-late/late.json" \
    --deadline "$OUT/deadline/deadline.json" \
    --rowcap "$OUT/rowcap/result.json" \
    --bytecap "$OUT/bytecap/result.json" \
    --setup "$OUT/setup/node-info.json" \
    --stats "$OUT/stats/http.json" \
    --server-audit "$OUT/server-audit/server-audit.json" \
    --block-cast "$OUT/block-cast/blocked.json" \
    --audit-dir "$OUT" \
    --out "$OUT/judge/verdict.json"
fi
fail=0
for name in setup observe decide visibility reconcile negative shape cancel cancel-late deadline rowcap bytecap block-cast server-audit decode judge; do
  got=$(cat "$OUT/$name/exit" 2>/dev/null || printf 'missing')
  if [ "$got" != "0" ]; then
    echo "phase $name $got"
    fail=1
  fi
done
if [ ! -f "$OUT/judge/verdict.json" ] || ! grep -q '"passed": true' "$OUT/judge/verdict.json"; then
  echo "judge did not pass"
  fail=1
fi
printf 'done\n' >"$OUT/finished"
if [ "$fail" != "0" ]; then
  printf 'phases\n' >"$OUT/stopped"
  exit 2
fi
exit 0
