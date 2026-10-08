#!/bin/bash
# After the frozen restore, an identity sequence must withhold the cut.
# It must not be omitted from the fence.
set -uo pipefail

if [ -z "${PGNAME:-}" ] || [ -z "${BIN:-}" ] || [ -z "${ROOT:-}" ] || [ -z "${EVIDENCE:-}" ] || [ -z "${TOOLDIR:-}" ]; then
  echo "identity.sh is run by run-live.sh" >&2
  exit 2
fi

IDENT=${EVIDENCE}/identity
mkdir -p "$IDENT"
sql_out() {
  db=$1
  dest=$2
  sql=$3
  printf '%s\n' "$sql" > "$ROOT/run/ident-query.sql"
  docker exec -u 0 "$PGNAME" /var/lib/postgresql/a391-work/psql-exec.sh "$db" ident-query.sql > "$dest" 2> "${dest}.err"
}

run_tool() {
  docker exec -u 0 "$PGNAME" \
    /var/lib/postgresql/a391-glibc/ld-linux-aarch64.so.1 \
    --library-path /var/lib/postgresql/a391-glibc \
    "$BIN" "$@"
}

sql_out oo_a356_agent "$IDENT/before.txt" "DROP TABLE IF EXISTS shop.a391_ident; CREATE TABLE shop.a391_ident (id bigint GENERATED ALWAYS AS IDENTITY); ALTER TABLE shop.a391_ident OWNER TO oo_a377_owner; SELECT n.nspname || '.' || c.relname || '|' || pg_get_userbyid(c.relowner) || '|' || d.deptype::text FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace JOIN pg_catalog.pg_depend d ON d.objid = c.oid AND d.classid = 'pg_catalog.pg_class'::regclass AND d.deptype = 'i' WHERE c.relkind = 'S' AND c.relname LIKE '%a391_ident%' ORDER BY 1;"
sql_out postgres "$IDENT/dbs-before.txt" "SELECT datname FROM pg_database WHERE datname LIKE 'oo_a356_restore%' OR datname LIKE 'oo_a391%' ORDER BY 1"

cat > "$ROOT/run/operator-identity.json" <<EOF
{
  "backup_dir": "/var/lib/postgresql/a391-backup-identity",
  "caller": "trusted_local_admin",
  "host": "/var/run/postgresql",
  "lineage_version": "pg-backup-restore/v1",
  "maintenance_database": "postgres",
  "owner": "a356",
  "password_file": "/var/lib/postgresql/a391-admin.pass",
  "port": "5432",
  "report_path": "/var/lib/postgresql/a391-work/identity-readiness.json",
  "restore_databases": ["oo_a356_restore_control", "oo_a356_restore_agent"],
  "schema_root": "/schemas",
  "schemas": [
    {"database": "oo_a356_control", "file": "tools/cdc-offset-store/migrations/oo_cdc_offset_control_v1.sql", "id": "cdc-offset-store-v1"},
    {"database": "oo_a356_control", "file": "tools/session-store/migrations/oo_session_control_v1.sql", "id": "session-store-v1"},
    {"database": "oo_a356_control", "file": "tools/calendar-store/migrations/oo_calendar_control_v1.sql", "id": "calendar-store-v1"},
    {"database": "oo_a356_agent", "file": "tools/enum-store/migrations/oo_enum_control_v1.sql", "id": "enum-store-v1"}
  ],
  "session": "eef831aa-dba4-4af2-a78b-541bc9e0a356",
  "source_cut": "cut-a356-1",
  "source_databases": ["oo_a356_control", "oo_a356_agent"],
  "target_fence": "fence-a356-restore",
  "tool_dir": "${TOOLDIR}",
  "user": "oo_a356_admin",
  "work_dir": "/var/lib/postgresql/a391-work"
}
EOF
docker exec -u 0 "$PGNAME" mkdir -p /var/lib/postgresql/a391-backup-identity
set +e
run_tool backup /run-in/operator-identity.json > "$IDENT/backup.out" 2> "$IDENT/backup.err"
backup_code=$?
set +e
printf '%s\n' "$backup_code" > "$IDENT/backup.exit"
docker cp "$PGNAME":/var/lib/postgresql/a391-backup-identity/manifest.json "$IDENT/manifest.json" 2>/dev/null || true
set +e
run_tool restore /run-in/operator-identity.json > "$IDENT/restore.out" 2> "$IDENT/restore.err"
restore_code=$?
printf '%s\n' "$restore_code" > "$IDENT/restore.exit"
docker cp "$PGNAME":/var/lib/postgresql/a391-work/identity-readiness.json "$IDENT/readiness.json" 2>/dev/null || true
sql_out postgres "$IDENT/dbs-after.txt" "SELECT datname FROM pg_database WHERE datname LIKE 'oo_a356_restore%' OR datname LIKE 'oo_a391%' ORDER BY 1"
python3 - <<'PY' "$IDENT/manifest.json" "$IDENT/manifest-fields.txt"
import json, pathlib, sys
src, dest = sys.argv[1:]
path = pathlib.Path(src)
out = pathlib.Path(dest)
if not path.exists():
    out.write_text("manifest_absent\n")
    raise SystemExit(0)
data = json.loads(path.read_text())
fence = data.get("sequence_fence", {})
lines = [
    f"cut_mode={data.get('cut_mode')}",
    f"history_insufficient={data.get('history_insufficient')}",
    f"held={fence.get('held')}",
    f"method={fence.get('method')}",
    f"lock_mode={fence.get('lock_mode')}",
]
for row in fence.get("sequences", []):
    lines.append(f"seq {row.get('database')} {row.get('relation')} {row.get('sqlstate')} held={row.get('held')}")
out.write_text("\n".join(lines) + "\n")
PY
echo "identity backup=${backup_code} restore=${restore_code}"
