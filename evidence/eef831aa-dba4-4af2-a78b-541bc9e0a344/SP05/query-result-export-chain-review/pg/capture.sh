#!/bin/bash
# Read the six source rows and the matching target numeric columns.
# Origin: a340 pg/capture.sh 30a53a36c223219a638ed7e56afff9642fa244002b51d02cb477702e08eda9eb
# Changes: owner a344. No OID catalog and no password on the command line.
set -u
NAME=oo-a344-pg
OUT=/Users/redux/puppy-one/open-ontology/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a344/SP05/query-result-export-chain-review/pg
mkdir -p "$OUT/actual"
if ! docker exec -u postgres "$NAME" pg_isready -U oo_fixture_admin -d postgres >/dev/null 2>&1; then
  echo "postgres is not ready" >&2
  printf '%s\n' 1 > "$OUT/actual/capture.exit"
  exit 1
fi
docker exec -u postgres "$NAME" psql -U oo_fixture_admin -d oo_a327_src -v ON_ERROR_STOP=1 -At -c \
  "SELECT entry_id::text || '|' || COALESCE(amount::text, 'NULL') || '|' || COALESCE(wide::text, 'NULL') FROM shop.entry ORDER BY entry_id" \
  > "$OUT/actual/source-rows.txt"
src_exit=$?
docker exec -u postgres "$NAME" psql -U oo_fixture_admin -d oo_a327_dst -v ON_ERROR_STOP=1 -At -F '|' -c \
  "SELECT logical_column, physical_name, scope_namespace_physical, scope_table_physical FROM _oo.physical_mappings WHERE logical_namespace = 'shop' AND logical_table = 'entry' ORDER BY logical_column" \
  > "$OUT/actual/mapping-entry.txt"
map_exit=$?
python3 - "$OUT/actual/mapping-entry.txt" <<'PY' > "$OUT/actual/target-select.sql"
import pathlib, sys
text = pathlib.Path(sys.argv[1]).read_text()
rows = {}
table = None
namespace = None
for line in text.splitlines():
    parts = line.split("|")
    if len(parts) != 4:
        continue
    logical, physical, scope_ns, scope_table = parts
    if logical == "":
        continue
    if not all(part.replace("_", "").isalnum() for part in (logical, physical, scope_ns, scope_table)):
        raise SystemExit("mapping identifier is not plain")
    rows[logical] = physical
    table = scope_table
    # scope_namespace_physical is not the PostgreSQL schema. The frozen
    # target stores the physical table in _oo.
    namespace = "_oo"
need = ("entry_id", "amount", "wide")
if table is None or namespace is None or any(name not in rows for name in need):
    raise SystemExit("mapping is missing entry_id, amount, or wide")
print(
    "SELECT {entry}::text || '|' || COALESCE({amount}::text, 'NULL') || '|' || COALESCE({wide}::text, 'NULL') "
    "FROM {namespace}.{table} ORDER BY {entry}".format(
        entry=rows["entry_id"],
        amount=rows["amount"],
        wide=rows["wide"],
        namespace=namespace,
        table=table,
    )
)
PY
sql_exit=$?
if [ "$sql_exit" -ne 0 ]; then
  printf '%s\n' "$sql_exit" > "$OUT/actual/capture.exit"
  exit "$sql_exit"
fi
docker exec -u postgres -i "$NAME" psql -U oo_fixture_admin -d oo_a327_dst -v ON_ERROR_STOP=1 -At \
  < "$OUT/actual/target-select.sql" \
  > "$OUT/actual/target-rows.txt"
tgt_exit=$?
if [ "$src_exit" -ne 0 ] || [ "$map_exit" -ne 0 ] || [ "$tgt_exit" -ne 0 ]; then
  printf '%s\n' 1 > "$OUT/actual/capture.exit"
  echo "capture_exit=1 src=$src_exit map=$map_exit tgt=$tgt_exit"
  exit 1
fi
printf '%s\n' 0 > "$OUT/actual/capture.exit"
echo "capture_exit=0"
exit 0
