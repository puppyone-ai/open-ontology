#!/bin/bash
# Host side of the five-column chain. The product Arrow read is ch-reader.
set -u
ROOT="/Users/redux/puppy-one/open-ontology"
BASE="/tmp/oo-dl01w1-40-bind"
OUT="$BASE/out/raw"
SCRIPTS="$ROOT/tools/clickhouse-preview/scripts"
log() { echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) $*" | tee -a "$BASE/out/coordinate.log"; }
rust_running() {
  docker inspect -f '{{.State.Running}}' oo-dl01w1-40-rust 2>/dev/null | grep -qx true
}
fail_bind() {
  log "bind failed: $1"
  printf '%s\n' "$1" > "$OUT/BIND_FAIL"
  exit 1
}

log "source-side"
python3 "$SCRIPTS/source_customer.py" > "$OUT/source-side.py.stdout" 2> "$OUT/source-side.py.stderr" &
source_pid=$!

start=$(date +%s)
while [ ! -f "$OUT/READER_BUILT" ]; do
  if ! rust_running; then
    log "rust stopped before reader build"
    wait "$source_pid" || true
    exit 1
  fi
  if [ $(( $(date +%s) - start )) -ge 5400 ]; then
    log "reader build timeout"
    printf '%s\n' timeout > "$OUT/BIND_FAIL"
    wait "$source_pid" || true
    exit 1
  fi
  sleep 5
done
log "reader built"
start=$(date +%s)
while [ ! -f "$OUT/FIXTURE_OK" ]; do
  if [ -f "$OUT/COUNT_MISMATCH" ]; then
    fail_bind "count-mismatch"
  fi
  if ! rust_running; then
    log "rust stopped during fixture"
    wait "$source_pid" || true
    exit 1
  fi
  if [ $(( $(date +%s) - start )) -ge 300 ]; then
    fail_bind "fixture-timeout"
  fi
  sleep 2
done
log "capture arrow"
set +e
docker exec oo-dl01w1-40-rust \
  /target/driver/debug/ch-reader arrow \
  --endpoint http://oo-dl01w1-40-clickhouse:8123 \
  --query-id oo-dl01w1-40-customer-five \
  --body /plan/customer-arrow.sql \
  --out /out/raw/customer-arrow.json \
  --body-out /out/raw/customer-arrow.body \
  --ipc-out /out/raw/customer-five.arrow \
  --timeout-ms 20000 \
  --user-file /run/ch/reader.user \
  --password-file /run/ch/reader.password \
  --row-cap 100
capture_code=$?
echo "$capture_code" > "$OUT/customer-arrow.exit"
if [ "$capture_code" -ne 0 ]; then
  fail_bind "ch-reader-$capture_code"
fi

python3 - <<'PY'
import json
from pathlib import Path
base = Path("/tmp/oo-dl01w1-40-bind")
report_path = base / "inputs/report.json"
report = json.loads(report_path.read_text())
digest = (base / "out/raw/ch-reader.sha").read_text().split()[0]
report["driver"]["digest"] = digest
report_path.write_text(json.dumps(report, indent=2) + "\n")
PY

docker exec oo-dl01w1-40-rust \
  /admission/oo-source-admission validate \
  --report /inputs/report.json \
  --config /inputs/config.json \
  > "$BASE/inputs/decision.stdout" 2> "$OUT/customer-validate.stderr"
validate_code=$?
echo "$validate_code" > "$OUT/customer-validate.exit"
if [ "$validate_code" -ne 0 ]; then
  fail_bind "validate-$validate_code"
fi
cp "$BASE/inputs/decision.stdout" "$OUT/customer-decision.stdout"

python3 "$SCRIPTS/bind_customer.py" \
  --report "$BASE/inputs/report.json" \
  --config "$BASE/inputs/config.json" \
  --catalog "$BASE/inputs/catalog.json" \
  --capture "$OUT/customer-arrow.json" \
  --receipt "$BASE/inputs/admission-receipt.json" \
  --decision "$BASE/inputs/decision.stdout" \
  --arrow "$OUT/customer-five.arrow" \
  --count-file "$OUT/independent-count.txt" \
  --driver-sha-file "$OUT/ch-reader.sha" \
  --owner 4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de \
  --host oo-dl01w1-40-pg \
  --report-container /inputs/report.json \
  --config-container /inputs/config.json \
  --catalog-container /plan/catalog.json \
  --capture-container /out/raw/customer-arrow.json \
  --receipt-container /inputs/admission-receipt.json \
  --decision-container /inputs/decision.stdout \
  --arrow-container /out/raw/customer-five.arrow \
  --ca-container /run/pg-secrets/pg-ca.crt \
  --password-container /run/pg-secrets/pg_admin_password \
  --registry-out "$BASE/plan/registry.json" \
  --positive-out "$BASE/plan/manifest-positive.json" \
  --negative-out "$BASE/plan/manifest-negative.json" \
  > "$OUT/registry-prepare.txt" 2> "$OUT/registry-prepare.stderr"
bind_code=$?
echo "$bind_code" > "$OUT/registry-prepare.exit"
if [ "$bind_code" -ne 0 ]; then
  fail_bind "registry-$bind_code"
fi
printf '%s\n' ready > "$OUT/BIND_READY"
log "bind ready"

start=$(date +%s)
while [ ! -f "$OUT/SEALED_OK" ]; do
  if ! rust_running; then
    log "rust stopped before seal"
    printf '%s\n' rust-ended > "$OUT/PROVISION_FAIL"
    wait "$source_pid" || true
    exit 1
  fi
  if [ $(( $(date +%s) - start )) -ge 7200 ]; then
    log "seal timeout"
    printf '%s\n' timeout > "$OUT/PROVISION_FAIL"
    exit 1
  fi
  sleep 5
done
log "authorities"
python3 - <<'PY'
import json, os, shutil
from pathlib import Path
out = Path("/tmp/oo-dl01w1-40-bind/out/raw")
sealed = json.loads(Path("/tmp/oo-dl01w1-40-bind/out/builder/authority.json").read_text())
policy = json.loads(Path("/tmp/oo-dl01w1-40-bind/out/builder/policy.json").read_text())
names = [column["name"] for column in policy["table"]["columns"]]
if names != ["reading_id", "ts", "amount", "note", "qty"]:
    raise SystemExit(f"policy columns {names}")
widened = json.loads(json.dumps(policy))
widened["table"]["columns"].append({
    "name": "version",
    "pii": {"class": "none"},
    "semantic_type": "identifier",
    "status": "confirmed",
})
widen_dir = out / "widen"
widen_dir.mkdir(parents=True, exist_ok=True)
(widen_dir / "policy.json").write_text(json.dumps(widened) + "\n")
widen_auth = json.loads(json.dumps(sealed))
widen_auth["policy_path"] = "/out/raw/widen/policy.json"
runtime = out / "authorities"
runtime.mkdir(parents=True, exist_ok=True)

def write_authority(name, doc):
    body = json.loads(json.dumps(doc))
    audit = out / "audit" / name
    if audit.exists():
        shutil.rmtree(audit)
    audit.mkdir(parents=True)
    os.chmod(audit, 0o700)
    body["audit_dir"] = f"/out/raw/audit/{name}"
    (runtime / f"{name}.json").write_text(json.dumps(body) + "\n")

for name in ("describe", "guard", "query", "cancel", "after-off", "reopen"):
    write_authority(name, sealed)
write_authority("widen", widen_auth)
(out / "AUTHORITIES_OK").write_text("ok\n")
PY
log "provision"
if ! python3 "$SCRIPTS/provision_customer.py" > "$OUT/provision.py.stdout" 2> "$OUT/provision.py.stderr"; then
  printf '%s\n' failed > "$OUT/PROVISION_FAIL"
  log "provision failed"
  wait "$source_pid" || true
  exit 1
fi
start=$(date +%s)
while [ ! -f "$OUT/query.stdout" ]; do
  if ! rust_running; then
    log "rust stopped before query"
    break
  fi
  if [ $(( $(date +%s) - start )) -ge 300 ]; then
    log "query timeout"
    break
  fi
  sleep 1
done
if [ -f "$OUT/query.stdout" ] && [ -f "$OUT/source-rows.stdout" ]; then
  if python3 "$SCRIPTS/row_oracle.py" \
    --profile customer \
    --operator "$OUT/query.stdout" \
    --source "$OUT/source-rows.stdout" \
    --verdict "$OUT/customer-verdict.txt" \
    > "$OUT/oracle.py.stdout" 2> "$OUT/oracle.py.stderr"; then
    log "oracle match"
  else
    log "oracle mismatch"
  fi
fi
start=$(date +%s)
while [ ! -f "$OUT/NEED_LOCK" ]; do
  if ! rust_running; then
    log "rust stopped before lock"
    wait "$source_pid" || true
    exit 1
  fi
  if [ $(( $(date +%s) - start )) -ge 180 ]; then
    log "need-lock timeout"
    wait "$source_pid" || true
    exit 1
  fi
  sleep 0.5
done
table=$(head -1 "$OUT/NEED_LOCK")
log "lock $table"
python3 "$ROOT/evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/raw/run/product-run-10-wal/command/lock_hold.py" "$table"
lock_code=$?
log "lock helper $lock_code"
start=$(date +%s)
while [ ! -f "$OUT/NEED_SOURCE_OFF" ]; do
  if ! rust_running; then
    log "rust stopped before source-off"
    wait "$source_pid" || true
    exit 1
  fi
  if [ $(( $(date +%s) - start )) -ge 180 ]; then
    log "source-off request timeout"
    wait "$source_pid" || true
    exit 1
  fi
  sleep 0.5
done
wait "$source_pid" || true
log "stop clickhouse"
docker stop oo-dl01w1-40-clickhouse > "$OUT/source-off.stdout" 2> "$OUT/source-off.stderr"
echo "$?" > "$OUT/source-off.exit"
docker inspect -f '{{.State.Status}}' oo-dl01w1-40-clickhouse > "$OUT/source-off-status.txt" || true
printf '%s\n' stopped > "$OUT/SOURCE_STOPPED"
exit "$lock_code"
