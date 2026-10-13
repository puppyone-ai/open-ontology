#!/bin/bash
# Capture the approved shipment documents from the existing Mongo container.
# Does not create a volume, does not replace the frozen Arrow, does not write source data.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/testing/dl01-driver-preflight/mongodb"
EVIDENCE="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product"
OUT="${CAPTURE_OUT:-$EVIDENCE/raw/repair-62-doc}"
SECRETS="/tmp/oo-dl01w1-41-secrets"
MONGO="oo-dl01w1-41-mongodb"
ADMIT="$SRC/scripts/admit.py"
CUSTOMER="$ROOT/tools/mongodb-preview/examples/customer.shipments.json"
DBA_URI="mongodb://127.0.0.1:27017/admin?appName=oo-dl01w1-41-dba&maxPoolSize=1"

phase() {
  mkdir -p "$OUT"
  printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" | tee -a "$OUT/phase.txt"
}

fail() {
  phase "FAILED $*"
  printf 'FAILED %s\n' "$*" > "$OUT/status.txt"
  python3 "$ADMIT" release mongodb >/dev/null 2>&1 || true
  exit 1
}

if [[ -e "$OUT" ]]; then
  echo "fresh directory already exists: $OUT" >&2
  exit 2
fi
mkdir -p "$OUT"
: > "$OUT/phase.txt"
phase "fulldoc capture start"
test -f "$CUSTOMER"
test -f "$SECRETS/reader"
test -f "$SECRETS/dba"
docker inspect "$MONGO" >/dev/null

if ! python3 "$ADMIT" acquire mongodb "$OUT/admission-mongodb.json"; then
  fail "mongodb capacity"
fi
if [[ "$(docker inspect -f '{{.State.Status}}' "$MONGO")" != "running" ]]; then
  docker start "$MONGO" >"$OUT/mongo-start.out"
fi

ready=0
ping_eval='const fs=require("fs"); const pw=fs.readFileSync("/run/secrets/dba","utf8").trim(); const admin=db.getSiblingDB("admin"); if (!admin.auth("oo_dl01w1_41_dba", pw)) quit(2); const r=admin.runCommand({ping:1}); if (r.ok===1) quit(0); quit(2);'
for _attempt in $(seq 1 20); do
  sleep 2
  set +e
  docker exec -u 0 -e HOME=/tmp "$MONGO" \
    mongosh --quiet --norc "$DBA_URI" \
    --eval "$ping_eval" \
    >"$OUT/ping.out" 2>"$OUT/ping.err"
  ping_rc=$?
  set -e
  if [[ "$ping_rc" -eq 0 ]]; then
    ready=1
    break
  fi
done
printf '%s\n' "$ready" >"$OUT/ping.ready"
if [[ "$ready" -ne 1 ]]; then
  fail "mongodb ping"
fi

phase "reader capture"
docker exec -u 0 "$MONGO" mkdir -p /tmp/oo-w141-capture
docker cp "$CUSTOMER" "$MONGO:/tmp/oo-w141-capture/customer.json"
docker cp "$SRC/scripts/capture-fulldoc.js" "$MONGO:/tmp/oo-w141-capture/capture-fulldoc.js"
set +e
docker exec -u 0 -e HOME=/tmp "$MONGO" \
  mongosh --quiet --norc "mongodb://127.0.0.1:27017/admin?appName=oo-dl01w1-41-reader&maxPoolSize=1" \
  --file /tmp/oo-w141-capture/capture-fulldoc.js \
  >"$OUT/capture.out" 2>"$OUT/capture.err"
capture_rc=$?
set -e
printf '%s\n' "$capture_rc" >"$OUT/capture.exit"
docker cp "$MONGO:/tmp/oo-w141-capture/status.txt" "$OUT/capture-status.txt" || true
docker cp "$MONGO:/tmp/oo-w141-capture/pre-read-deny.json" "$OUT/pre-read-deny.json" || true
docker cp "$MONGO:/tmp/oo-w141-capture/capture.json" "$OUT/capture.json" || true
docker cp "$MONGO:/tmp/oo-w141-capture/full-approved.bson.hex" "$OUT/full-approved.bson.hex" || true
docker cp "$MONGO:/tmp/oo-w141-capture/seal-scope.bson.hex" "$OUT/seal-scope.bson.hex" || true
docker exec -u 0 "$MONGO" rm -rf /tmp/oo-w141-capture || true
if [[ "$capture_rc" -ne 0 ]]; then
  fail "capture $capture_rc"
fi
phase "capture exit 0"
python3 "$ADMIT" release mongodb >/dev/null
phase "mongodb slot released container left running"
