#!/bin/bash
# Re-read the already recorded profile and map logical doc. No new DML.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/testing/dl01-driver-preflight/mongodb"
EVIDENCE="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/c5-cycle-79"
MONGO="oo-dl01w1-41-mongodb"
PG="oo-dl01w1-41-pg"

phase() { printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" | tee -a "$EVIDENCE/phase.txt"; }

docker cp "$SRC/scripts/c5-live.js" "$MONGO:/tmp/oo-w141-c5/c5-live.js"
phase "named profile reread"
set +e
docker exec -u 0 -e HOME=/tmp -e C5_MODE=audit -e C5_OUT=/tmp/oo-w141-c5/audit-profiled-named.json \
  "$MONGO" mongosh --quiet --norc \
  "mongodb://127.0.0.1:27017/admin?appName=oo-dl01w1-41-c5-audit-named&maxPoolSize=1" \
  --file /tmp/oo-w141-c5/c5-live.js \
  >"$EVIDENCE/audit-profiled-named.json.stdout" 2>"$EVIDENCE/audit-profiled-named.json.stderr"
audit_rc=$?
set -e
printf '%s\n' "$audit_rc" >"$EVIDENCE/audit-profiled-named.exit"
docker cp "$MONGO:/tmp/oo-w141-c5/audit-profiled-named.json" "$EVIDENCE/audit-profiled-named.json" || true
[[ "$audit_rc" -eq 0 ]]

python3 - "$EVIDENCE" << 'PY'
import json, pathlib, sys
root = pathlib.Path(sys.argv[1])
audit = json.loads((root / "audit-profiled-named.json").read_text())
by = {row.get("comment"): row for row in audit["rows"]}
assert by["oo-dl01w1-41:c5:dba:audit-insert"]["command"] == "insert"
assert by["oo-dl01w1-41:c5:dba:audit-delete"]["command"] == "delete"
assert by["oo-dl01w1-41:c5:dba:audit-update"]["command"] == "update"
reader = by["oo-dl01w1-41:c5:reader:audit-read"]
assert reader["command"] == "find"
assert "oo_dl01w1_41_reader" in reader["user"]
assert audit["reader_dml"] == 0
for comment in (
    "oo-dl01w1-41:c5:dba:audit-insert",
    "oo-dl01w1-41:c5:dba:audit-delete",
    "oo-dl01w1-41:c5:dba:audit-update",
):
    assert "oo_dl01w1_41_dba" in by[comment]["user"]
print("named-ok", len(audit["rows"]))
PY

phase "map logical doc"
docker exec "$PG" psql -v ON_ERROR_STOP=1 -U oo_w141_admin -d oo_w141 -At -c \
  "SELECT 'doc=' || format_type(a.atttypid, a.atttypmod) || ' oid=' || a.atttypid || ' typmod=' || a.atttypmod || ' physical=' || m.physical_name || ' generation=' || m.run_generation || ' logical=' || m.logical_namespace || '.' || m.logical_table || '.' || m.logical_column FROM _oo.physical_mappings m JOIN pg_class t ON t.relname = m.scope_table_physical JOIN pg_namespace tn ON tn.oid = t.relnamespace AND tn.nspname = '_oo' JOIN pg_attribute a ON a.attrelid = t.oid AND a.attname = m.physical_name AND a.attnum > 0 AND NOT a.attisdropped WHERE m.level = 'c' AND m.logical_column = 'doc' AND m.logical_table = 'shipments' AND m.scope_table_physical LIKE 'pt_4d114%'" \
  >"$EVIDENCE/pg-gen2-doc-mapped.txt"
grep -q '^doc=jsonb oid=3802 typmod=-1 ' "$EVIDENCE/pg-gen2-doc-mapped.txt"

nonnull_sql=$(docker exec "$PG" psql -v ON_ERROR_STOP=1 -U oo_w141_admin -d oo_w141 -At -c \
  "SELECT format('SELECT ''doc_nonnull='' || count(*) FILTER (WHERE %I IS NOT NULL) FROM %I.%I', m.physical_name, '_oo', m.scope_table_physical) FROM _oo.physical_mappings m WHERE m.level = 'c' AND m.logical_column = 'doc' AND m.logical_table = 'shipments' AND m.scope_table_physical LIKE 'pt_4d114%'")
docker exec "$PG" psql -v ON_ERROR_STOP=1 -U oo_w141_admin -d oo_w141 -At -c "$nonnull_sql" >"$EVIDENCE/pg-gen2-doc-nonnull.txt"
grep -qx 'doc_nonnull=16' "$EVIDENCE/pg-gen2-doc-nonnull.txt"

phase "stop mongo keep volume"
docker stop "$MONGO" >"$EVIDENCE/mongo-stop.stdout"
docker inspect -f '{{.Name}} {{.State.Status}} {{.State.ExitCode}}' "$MONGO" "$PG" >"$EVIDENCE/resource-final.txt"
phase "name and doc map done"
