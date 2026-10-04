#!/bin/bash
# Second measurement on the same database. Records SQLSTATE and ACLs.
# Does not reinstall and does not claim to be the compiler.
set -u
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a169
REPO=/Users/redux/puppy-one/open-ontology
PG=${REPO}/evidence/${RUN_ID}/SP06/pg-policy-compiler/pg
OUT=${PG}/out-sqlstate
SEC=/tmp/oo-a169-${RUN_ID}/secrets
NAME=oo-a169-pg
mkdir -p "$OUT"

scrub() {
  python3 - "$OUT" "$SEC/admin.pass" "$SEC/login.pass" <<'PY'
import pathlib, sys
out = pathlib.Path(sys.argv[1])
needles = [pathlib.Path(p).read_text(encoding="utf-8").strip().encode() for p in sys.argv[2:]]
needles = [n for n in needles if n]
for item in out.rglob("*"):
    if not item.is_file():
        continue
    data = item.read_bytes()
    updated = data
    for needle in needles:
        updated = updated.replace(needle, b"[REDACTED]")
    if updated != data:
        item.write_bytes(updated)
PY
}

docker exec -u postgres -i "$NAME" bash -s <<'EOS' > "$OUT/admin.stdout" 2> "$OUT/admin.stderr"
set -u
export PGPASSWORD="$(cat /secrets/admin.pass)"
export PGSSLMODE=disable
psql -h 127.0.0.1 -U oo_fixture_admin -d oo_a169 -v ON_ERROR_STOP=1 -X -q <<'SQL'
SELECT n.nspname, c.relname, c.relowner::regrole AS owner, c.relacl
FROM pg_class AS c
JOIN pg_namespace AS n ON n.oid = c.relnamespace
WHERE n.nspname = '_oo'
  AND c.relname IN (
    'probe_orders', 'physical_mappings', 'full_import_session',
    'full_import_run', 'full_import_table', 'full_import_quality'
  )
ORDER BY c.relname;
SELECT a.attname, a.attacl
FROM pg_attribute AS a
JOIN pg_class AS c ON c.oid = a.attrelid
JOIN pg_namespace AS n ON n.oid = c.relnamespace
WHERE n.nspname = '_oo' AND c.relname = 'probe_orders' AND a.attnum > 0 AND NOT a.attisdropped
ORDER BY a.attnum;
SELECT to_regclass('_oo.full_import_run') AS import_run,
       to_regclass('_oo.full_import_session') AS import_session,
       to_regclass('_oo.physical_mappings') AS mappings,
       has_table_privilege('oo_r_a', '_oo.full_import_run', 'SELECT') AS run_select,
       has_table_privilege('oo_r_a', '_oo.full_import_session', 'SELECT') AS session_select,
       has_table_privilege('oo_r_a', '_oo.full_import_table', 'SELECT') AS table_select,
       has_schema_privilege('oo_r_a', '_oo', 'USAGE') AS usage,
       has_schema_privilege('oo_a169_login', '_oo', 'USAGE') AS login_usage;
SQL
status=$?
unset PGPASSWORD
exit "$status"
EOS
printf '%s\n' "$?" > "$OUT/admin.exit"

docker exec -u postgres -i "$NAME" bash -s <<'EOS' > "$OUT/reader.stdout" 2> "$OUT/reader.stderr"
set -u
umask 077
pass=$(cat /secrets/login.pass)
printf '%s\n' "oo-a169-pg:5432:oo_a169:oo_a169_login:${pass}" > /tmp/oo_a169.pgpass
export PGPASSFILE=/tmp/oo_a169.pgpass
export PGSSLMODE=verify-full
export PGSSLROOTCERT=/tls-in/ca.crt
psql -h oo-a169-pg -U oo_a169_login -d oo_a169 -v ON_ERROR_STOP=off -v VERBOSITY=verbose -X <<'SQL'
\set ON_ERROR_STOP off
SELECT pg_backend_pid() AS pid;
BEGIN;
SET LOCAL ROLE oo_r_a;
SELECT secret FROM _oo.probe_orders;
ROLLBACK;
BEGIN;
SET LOCAL ROLE oo_r_a;
INSERT INTO _oo.probe_orders (id, region, secret) VALUES (9, 'eu', 'x');
ROLLBACK;
BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;
SET LOCAL ROLE oo_r_a;
SELECT extra FROM _oo.probe_orders;
ROLLBACK;
BEGIN;
SET LOCAL ROLE oo_r_a;
SELECT set_config('oo.claims', '{"region":["us"]}', true);
ROLLBACK;
BEGIN;
SET LOCAL ROLE oo_r_a;
SELECT count(*) FROM _oo.physical_mappings;
ROLLBACK;
BEGIN;
SET LOCAL ROLE oo_r_a;
SELECT count(*) FROM _oo.full_import_run;
ROLLBACK;
BEGIN;
SET LOCAL ROLE oo_r_b;
SELECT region FROM _oo.probe_orders;
ROLLBACK;
SELECT current_user AS back_to_login, pg_backend_pid() AS pid_after;
SQL
status=$?
rm -f /tmp/oo_a169.pgpass
unset pass
exit "$status"
EOS
printf '%s\n' "$?" > "$OUT/reader.exit"
scrub
if grep -R -F -f "$SEC/admin.pass" "$OUT" >/dev/null 2>&1 || grep -R -F -f "$SEC/login.pass" "$OUT" >/dev/null 2>&1; then
  echo "secret leaked into sqlstate output" >&2
  printf '%s\n' 1 > "$PG/sqlstate.exit"
  exit 1
fi
printf '%s\n' 0 > "$PG/sqlstate.exit"
echo "sqlstate_finished"
