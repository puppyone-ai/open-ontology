#!/bin/bash
# Real standalone C1 refusal and two C5 periods. Does not import generation 2.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/testing/dl01-driver-preflight/mongodb"
EVIDENCE="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/c5-cycle-79"
MONGO="oo-dl01w1-41-mongodb"
PG="oo-dl01w1-41-pg"
SECRETS="/tmp/oo-dl01w1-41-secrets"
PERIOD_A=3000
PERIOD_B=8000
DELETED="c141d04141eef0410000000f"
UPDATED="c141d04141eef04100000000"
INSERTED="c141d04141eef04100000010"

if [[ -e "$EVIDENCE" ]]; then
  echo "evidence already exists" >&2
  exit 2
fi
mkdir -p "$EVIDENCE"
exec > >(tee -a "$EVIDENCE/host.stdout") 2> >(tee -a "$EVIDENCE/host.stderr" >&2)

phase() {
  printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" | tee -a "$EVIDENCE/phase.txt"
}

run_js() {
  local mode="$1"
  local outfile="$2"
  local comment="${3:-}"
  docker exec -u 0 -e HOME=/tmp -e C5_MODE="$mode" -e C5_OUT="/tmp/oo-w141-c5/$outfile" -e C5_COMMENT="$comment" \
    "$MONGO" mongosh --quiet --norc \
    "mongodb://127.0.0.1:27017/admin?appName=oo-dl01w1-41-c5-${mode}&maxPoolSize=1" \
    --file /tmp/oo-w141-c5/c5-live.js \
    >"$EVIDENCE/${outfile}.stdout" 2>"$EVIDENCE/${outfile}.stderr"
}

phase "collect named resources"
{
  echo "builder_processes:"
  ps -ax -o pid,command | awk '/mongodb-preview|cargo (build|test|clippy)|oo-dl01w1-41-rust/ && !/awk/ {print}' || true
  echo "containers:"
  docker ps -a --filter name=oo-dl01w1-41 --format '{{.Names}} {{.Status}}'
  echo "volumes:"
  docker volume ls --filter name=oo-dl01w1-41 --format '{{.Name}}'
  echo "pg_cpu_max:"
  docker exec "$PG" sh -c 'cat /sys/fs/cgroup/cpu.max 2>/dev/null || echo missing'
} >"$EVIDENCE/resource-collection.txt"

test -f "$SECRETS/reader"
test -f "$SECRETS/dba"
test -f "$SECRETS/audit"
if [[ "$(docker inspect -f '{{.State.Status}}' "$MONGO")" != "running" ]]; then
  docker start "$MONGO" >"$EVIDENCE/mongo-start.stdout"
fi
phase "mongo running"

docker exec -u 0 "$MONGO" mkdir -p /tmp/oo-w141-c5
docker cp "$SRC/scripts/c5-live.js" "$MONGO:/tmp/oo-w141-c5/c5-live.js"

phase "cdc probe"
set +e
run_js cdc cdc.json
cdc_rc=$?
set -e
printf '%s\n' "$cdc_rc" >"$EVIDENCE/cdc.exit"
docker cp "$MONGO:/tmp/oo-w141-c5/cdc.json" "$EVIDENCE/cdc.json" || true
if [[ "$cdc_rc" -ne 0 ]]; then
  phase "FAILED cdc $cdc_rc"
  exit "$cdc_rc"
fi

phase "baseline read"
set +e
run_js read baseline.json "oo-dl01w1-41:c5:reader:baseline"
base_rc=$?
set -e
printf '%s\n' "$base_rc" >"$EVIDENCE/baseline.exit"
docker cp "$MONGO:/tmp/oo-w141-c5/baseline.json" "$EVIDENCE/baseline.json"
[[ "$base_rc" -eq 0 ]]
python3 - "$EVIDENCE/baseline.json" << 'PY'
import json, sys
body = json.load(open(sys.argv[1]))
assert body["count"] == 16, body["count"]
assert body["find_sent_unapproved"] is False
keys = [row["key"] for row in body["rows"]]
assert len(keys) == len(set(keys)) == 16
PY

phase "dba update and delete"
set +e
run_js dba-a dba-a.json
dba_a=$?
set -e
printf '%s\n' "$dba_a" >"$EVIDENCE/dba-a.exit"
docker cp "$MONGO:/tmp/oo-w141-c5/dba-a.json" "$EVIDENCE/dba-a.json"
[[ "$dba_a" -eq 0 ]]

python3 - "$EVIDENCE/period-a.json" "$PERIOD_A" << 'PY'
import json, sys, time
period = int(sys.argv[2])
start = time.time()
time.sleep(period / 1000)
end = time.time()
elapsed = int((end - start) * 1000)
json.dump({"period_ms": period, "elapsed_ms": elapsed, "start": start, "end": end}, open(sys.argv[1], "w"))
if elapsed < period:
    raise SystemExit(f"period short {elapsed}")
PY
phase "period A elapsed"

set +e
run_js read cycle-a.json "oo-dl01w1-41:c5:reader:cycle-a"
cycle_a=$?
set -e
printf '%s\n' "$cycle_a" >"$EVIDENCE/cycle-a.exit"
docker cp "$MONGO:/tmp/oo-w141-c5/cycle-a.json" "$EVIDENCE/cycle-a.json"
[[ "$cycle_a" -eq 0 ]]

phase "dba insert"
set +e
run_js dba-b dba-b.json
dba_b=$?
set -e
printf '%s\n' "$dba_b" >"$EVIDENCE/dba-b.exit"
docker cp "$MONGO:/tmp/oo-w141-c5/dba-b.json" "$EVIDENCE/dba-b.json"
[[ "$dba_b" -eq 0 ]]

python3 - "$EVIDENCE/period-b.json" "$PERIOD_B" << 'PY'
import json, sys, time
period = int(sys.argv[2])
start = time.time()
time.sleep(period / 1000)
end = time.time()
elapsed = int((end - start) * 1000)
json.dump({"period_ms": period, "elapsed_ms": elapsed, "start": start, "end": end}, open(sys.argv[1], "w"))
if elapsed < period:
    raise SystemExit(f"period short {elapsed}")
PY
phase "period B elapsed"

set +e
run_js read cycle-b.json "oo-dl01w1-41:c5:reader:cycle-b"
cycle_b=$?
set -e
printf '%s\n' "$cycle_b" >"$EVIDENCE/cycle-b.exit"
docker cp "$MONGO:/tmp/oo-w141-c5/cycle-b.json" "$EVIDENCE/cycle-b.json"
[[ "$cycle_b" -eq 0 ]]

set +e
run_js read cycle-b-again.json "oo-dl01w1-41:c5:reader:cycle-b-again"
again=$?
set -e
printf '%s\n' "$again" >"$EVIDENCE/cycle-b-again.exit"
docker cp "$MONGO:/tmp/oo-w141-c5/cycle-b-again.json" "$EVIDENCE/cycle-b-again.json"
[[ "$again" -eq 0 ]]

set +e
run_js keys keys.json
keys_rc=$?
set -e
printf '%s\n' "$keys_rc" >"$EVIDENCE/keys.exit"
docker cp "$MONGO:/tmp/oo-w141-c5/keys.json" "$EVIDENCE/keys.json"
[[ "$keys_rc" -eq 0 ]]

set +e
run_js audit audit.json
audit_rc=$?
set -e
printf '%s\n' "$audit_rc" >"$EVIDENCE/audit.exit"
docker cp "$MONGO:/tmp/oo-w141-c5/audit.json" "$EVIDENCE/audit.json"
[[ "$audit_rc" -eq 0 ]]

python3 - "$EVIDENCE" "$DELETED" "$UPDATED" "$INSERTED" << 'PY'
import json, pathlib, sys
root = pathlib.Path(sys.argv[1])
deleted, updated, inserted = sys.argv[2:]
base = json.loads((root / "baseline.json").read_text())
cycle_a = json.loads((root / "cycle-a.json").read_text())
cycle_b = json.loads((root / "cycle-b.json").read_text())
again = json.loads((root / "cycle-b-again.json").read_text())
keys = json.loads((root / "keys.json").read_text())
dba_b = json.loads((root / "dba-b.json").read_text())
audit = json.loads((root / "audit.json").read_text())
period_a = json.loads((root / "period-a.json").read_text())
period_b = json.loads((root / "period-b.json").read_text())
assert period_a["period_ms"] != period_b["period_ms"]
assert period_a["elapsed_ms"] >= period_a["period_ms"]
assert period_b["elapsed_ms"] >= period_b["period_ms"]
assert cycle_a["count"] == 15
assert deleted not in {row["key"] for row in cycle_a["rows"]}
assert inserted not in {row["key"] for row in cycle_a["rows"]}
a_updated = next(row for row in cycle_a["rows"] if row["key"] == updated)
base_updated = next(row for row in base["rows"] if row["key"] == updated)
assert a_updated["canonical"] != base_updated["canonical"]
assert "memo-c5-period" in a_updated["canonical"]
assert "hiddenNote" not in a_updated["canonical"]
assert cycle_b["count"] == 16
bkeys = {row["key"] for row in cycle_b["rows"]}
assert deleted not in bkeys
assert inserted in bkeys
assert again["rows"] == cycle_b["rows"]
assert dba_b["shipments"] == 16
assert dba_b["price_odd"] == 3
assert dba_b["mixed_marks"] == 2
assert "hiddenNote" in keys["fields"]
assert keys["values_retained"] is False
assert keys["field_types"]["hiddenNote"] == ["int", "string"] or keys["field_types"]["hiddenNote"] == ["string", "int"]
assert "weight" in keys["field_types"]
assert "double" in keys["field_types"]["weight"]
assert audit["reader_dml"] == 0
comments = [row["comment"] for row in audit["rows"]]
assert any(item and item.endswith(":dba:update") for item in comments)
assert any(item and item.endswith(":dba:delete") for item in comments)
assert any(item and item.endswith(":dba:insert") for item in comments)
reader_cmds = [row["command"] for row in audit["rows"] if "reader" in str(row.get("user"))]
assert "insert" not in reader_cmds and "update" not in reader_cmds and "delete" not in reader_cmds
print("oracle-ok", cycle_b["count"])
PY

phase "pg generations unchanged"
docker exec "$PG" psql -v ON_ERROR_STOP=1 -U oo_w141_admin -d oo_w141 -At -c \
  "SELECT 'gen1=' || count(*) FROM _oo.pt_a1d0e63667e64ab1b7b9c250e46e7043a798de705406b292dd4b238c3254" \
  >"$EVIDENCE/pg-gen1.txt"
docker exec "$PG" psql -v ON_ERROR_STOP=1 -U oo_w141_admin -d oo_w141 -At -c \
  "SELECT 'gen2=' || count(*) FROM _oo.pt_4d11431451ccacc3837c1cbd40cc4f0e28844c70b465650f63aef51b9898" \
  >"$EVIDENCE/pg-gen2.txt"
docker exec "$PG" psql -v ON_ERROR_STOP=1 -U oo_w141_admin -d oo_w141 -At -c \
  "SELECT 'doc=' || format_type(a.atttypid, a.atttypmod) FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='_oo' AND c.relname='pt_4d11431451ccacc3837c1cbd40cc4f0e28844c70b465650f63aef51b9898' AND a.attname='doc' AND NOT a.attisdropped" \
  >"$EVIDENCE/pg-gen2-doc.txt"
grep -qx 'gen1=16' "$EVIDENCE/pg-gen1.txt"
grep -qx 'gen2=16' "$EVIDENCE/pg-gen2.txt"
grep -qx 'doc=jsonb' "$EVIDENCE/pg-gen2-doc.txt"

phase "stop mongo keep volume"
docker stop "$MONGO" >"$EVIDENCE/mongo-stop.stdout"
docker inspect -f '{{.Name}} {{.State.Status}} {{.State.ExitCode}}' "$MONGO" "$PG" >"$EVIDENCE/resource-final.txt"
phase "live cycle done"
