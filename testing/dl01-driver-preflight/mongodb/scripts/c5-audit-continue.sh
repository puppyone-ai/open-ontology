#!/bin/bash
# Profile was 0 after the retained mongod start. This pass turns it on and
# records a separate DBA insert/delete/update beside a reader find.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/testing/dl01-driver-preflight/mongodb"
EVIDENCE="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/c5-cycle-79"
MONGO="oo-dl01w1-41-mongodb"
PG="oo-dl01w1-41-pg"

phase() { printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" | tee -a "$EVIDENCE/phase.txt"; }

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

docker cp "$SRC/scripts/c5-live.js" "$MONGO:/tmp/oo-w141-c5/c5-live.js"
phase "profiled identity pass"
set +e
run_js dba-audit dba-audit.json
dba_rc=$?
set -e
printf '%s\n' "$dba_rc" >"$EVIDENCE/dba-audit.exit"
docker cp "$MONGO:/tmp/oo-w141-c5/dba-audit.json" "$EVIDENCE/dba-audit.json" || true
[[ "$dba_rc" -eq 0 ]]

set +e
run_js read audit-read.json "oo-dl01w1-41:c5:reader:audit-read"
read_rc=$?
set -e
printf '%s\n' "$read_rc" >"$EVIDENCE/audit-read.exit"
docker cp "$MONGO:/tmp/oo-w141-c5/audit-read.json" "$EVIDENCE/audit-read.json"
[[ "$read_rc" -eq 0 ]]

set +e
run_js audit audit-profiled.json
audit_rc=$?
set -e
printf '%s\n' "$audit_rc" >"$EVIDENCE/audit-profiled.exit"
docker cp "$MONGO:/tmp/oo-w141-c5/audit-profiled.json" "$EVIDENCE/audit-profiled.json" || true
[[ "$audit_rc" -eq 0 ]]

python3 - "$EVIDENCE" << 'PY'
import json, pathlib, sys
root = pathlib.Path(sys.argv[1])
cycle_b = json.loads((root / "cycle-b.json").read_text())
again = json.loads((root / "audit-read.json").read_text())
audit = json.loads((root / "audit-profiled.json").read_text())
assert again["rows"] == cycle_b["rows"]
assert audit["reader_dml"] == 0
comments = [row.get("comment") for row in audit["rows"]]
users = {row.get("comment"): row.get("user") for row in audit["rows"]}
for suffix in (":dba:audit-insert", ":dba:audit-delete", ":dba:audit-update"):
    match = [row for row in audit["rows"] if str(row.get("comment") or "").endswith(suffix)]
    assert match, suffix
    assert "oo_dl01w1_41_dba" in str(match[0]["user"]), match[0]
reader = [row for row in audit["rows"] if "reader" in str(row.get("user"))]
assert reader, "reader profile missing"
assert all(row["command"] not in {"insert", "update", "delete"} for row in reader)
assert "c141d04141eef0410000000f" not in {row["key"] for row in again["rows"]}
print("audit-ok", len(audit["rows"]), "reader", len(reader))
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
phase "audit continue done"
