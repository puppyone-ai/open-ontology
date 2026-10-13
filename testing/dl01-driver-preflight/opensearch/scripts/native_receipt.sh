#!/bin/bash
# Produce capture receipts from the committed C5 cycle files.
# os-reader here is the receipt producer. driver_digest hashes the sealed capture ELF.
set -euo pipefail
ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/testing/dl01-driver-preflight/opensearch"
C5="$ROOT/evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-driver-preflight/c5-loop-20261012"
OUT="$ROOT/evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-driver-preflight/native-20261012"
IMAGE="oo-a395-selfbuild:local"
READER="$OUT/bin/os-reader"
DRIVER="$C5/bin/os-reader"
if [ ! -f "$READER" ] || [ ! -f "$DRIVER" ]; then
  echo "reader or capture driver missing" >&2
  exit 2
fi
run_fixed() {
  local name="$1" gen="$2" parent="$3" mark="$4" prior="$5"
  local dir="$OUT/receipts/$name"
  mkdir -p "$dir"
  local args=(
    c5-receipt
    --arrow "/capture/$name/batch.arrow"
    --audit "/capture/$name/audit.jsonl"
    --config /fixtures/c5-schedule.json
    --catalog "/capture/$name/live.json"
    --registry /fixtures/customer-request.json
    --driver /driver/os-reader
    --snapshot "/capture/$name/snapshot.json"
    --result "/capture/$name/result.json"
    --generation "$gen"
    --capture-watermark "$mark"
    --batch-id "os44-$name"
    --logical-schema ledger
    --logical-table ledger_rows
    --key-column source_doc_id
    --host 127.0.0.1
    --port 5432
    --database oo_dl01w1_44
    --runtime-principal oo_dl01w1_44_runtime
    --password-file /private/native-runtime.pw
    --arrow-path "/work/$name/batch.arrow"
    --admission-path "/work/$name/receipt.json"
    --out-receipt "/work/$name/receipt.json"
    --out-request "/work/$name/request.json"
  )
  if [ -n "$parent" ]; then
    args+=(--parent "$parent")
  fi
  if [ -n "$prior" ]; then
    args+=(--prior-snapshot "/capture/$prior/snapshot.json")
  fi
  set +e
  docker run --rm --network none --platform linux/arm64 --pull never \
    --user 0:0 --security-opt no-new-privileges --cap-drop ALL \
    --entrypoint /opt/os-reader \
    -v "$READER:/opt/os-reader:ro" \
    -v "$C5/proof:/capture:ro" \
    -v "$SRC/fixtures:/fixtures:ro" \
    -v "$DRIVER:/driver/os-reader:ro" \
    -v /tmp/oo-dl01w1-44-private:/private:ro \
    -v "$OUT/receipts:/work" \
    --label owner=oo-dl01w1-44 --label task=DL01-W1-44 \
    "$IMAGE" "${args[@]}" > "$dir/stdout.txt" 2> "$dir/stderr.txt"
  local code=$?
  set -e
  printf '%s\n' "$code" > "$dir/exit.txt"
  if [ "$code" != "0" ]; then
    echo "receipt $name exit $code" >&2
    exit "$code"
  fi
}
run_fixed fast-1 1 "" 1 ""
run_fixed fast-3 2 1 3 ""
run_fixed fast-5 3 2 5 ""
run_fixed fast-8 4 3 8 fast-5
echo RECEIPT_OK
