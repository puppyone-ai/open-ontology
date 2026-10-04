#!/bin/bash
# Database-fact probe. This is not the gateway policy compiler.
# It installs the reviewed migration files and measures column GRANT and SET.
set -u
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a169
REPO=/Users/redux/puppy-one/open-ontology
PG=${REPO}/evidence/${RUN_ID}/SP06/pg-policy-compiler/pg
OUT=${PG}/out
NAME=oo-a169-pg
SEC=/tmp/oo-a169-${RUN_ID}/secrets
mkdir -p "$OUT"

scrub_out() {
  python3 - "$OUT" "$SEC/admin.pass" "$SEC/login.pass" <<'PY'
import pathlib, sys
out = pathlib.Path(sys.argv[1])
needles = []
for path in sys.argv[2:]:
    text = pathlib.Path(path).read_text(encoding="utf-8").strip()
    if text:
        needles.append(text.encode())
if not out.exists():
    raise SystemExit(0)
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

# $1 name, $2 database, stdin is SQL. Password stays inside the container.
admin_sql() {
  local name="$1"
  local db="$2"
  local sql
  sql=$(cat)
  docker exec -u postgres -i "$NAME" bash -s -- "$db" <<EOS > "$OUT/${name}.stdout" 2> "$OUT/${name}.stderr"
set -u
db="\$1"
export PGPASSWORD="\$(cat /secrets/admin.pass)"
export PGSSLMODE=disable
psql -h 127.0.0.1 -U oo_fixture_admin -d "\$db" -v ON_ERROR_STOP=1 -X -q <<'SQL'
${sql}
SQL
status=\$?
unset PGPASSWORD
exit \$status
EOS
  local status=$?
  printf '%s\n' "$status" > "$OUT/${name}.exit"
  scrub_out
  return "$status"
}

run_login() {
  local name="$1"
  local sql="$2"
  docker exec -u postgres -i "$NAME" bash -s <<EOS > "$OUT/${name}.stdout" 2> "$OUT/${name}.stderr"
set -u
umask 077
pass=\$(cat /secrets/login.pass)
printf '%s\n' "oo-a169-pg:5432:oo_a169:oo_a169_login:\${pass}" > /tmp/oo_a169.pgpass
export PGPASSFILE=/tmp/oo_a169.pgpass
export PGSSLMODE=verify-full
export PGSSLROOTCERT=/tls-in/ca.crt
psql -h oo-a169-pg -U oo_a169_login -d oo_a169 -v ON_ERROR_STOP=1 -X -q <<'SQL'
${sql}
SQL
status=\$?
rm -f /tmp/oo_a169.pgpass
unset pass
exit \$status
EOS
  local status=$?
  printf '%s\n' "$status" > "$OUT/${name}.exit"
  scrub_out
  return 0
}

echo "=== reset partial cluster objects from an earlier attempt ==="
admin_sql reset_cluster postgres <<'SQL'
DROP DATABASE IF EXISTS oo_a169 WITH (FORCE);
DROP ROLE IF EXISTS oo_target_owner;
DROP ROLE IF EXISTS oo_pipeline_runtime;
SQL
if [ "$(cat "$OUT/reset_cluster.exit")" != 0 ]; then
  echo "reset_cluster failed" >&2
  printf '%s\n' 1 > "$PG/probe.exit"
  exit 1
fi

echo "=== create database ==="
admin_sql create_db postgres <<'SQL'
CREATE DATABASE oo_a169 TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8';
SQL
if [ "$(cat "$OUT/create_db.exit")" != 0 ]; then
  echo "create_db failed" >&2
  printf '%s\n' 1 > "$PG/probe.exit"
  exit 1
fi

echo "=== install reviewed migrations ==="
# 0000 creates roles and cannot sit in a transaction block.
# 0003 takes LOCK TABLE, so 0001/0003/0004 run in one transaction as
# oo_target_owner, matching target_installer::install. 0005 runs in a
# second transaction after the superuser stats grant, matching
# install_full_import. Checksums are the files' SHA-256, not a stored claim.
MIG_DIR="${REPO}/crates/oo-pipeline/migrations"
hash_file() { shasum -a 256 "$1" | awk '{print $1}'; }
BOOT_SHA=$(hash_file "$MIG_DIR/0000_bootstrap_roles.sql")
FENCE_SHA=$(hash_file "$MIG_DIR/0001_target_fence_expand.sql")
PRE_SHA=$(hash_file "$MIG_DIR/0003_target_segment_preflight_expand.sql")
MAP_SHA=$(hash_file "$MIG_DIR/0004_physical_mapping.sql")
FULL_SHA=$(hash_file "$MIG_DIR/0005_full_import_expand.sql")
admin_sql install oo_a169 <<SQL
\\set ON_ERROR_STOP on
\\i /migrations/0000_bootstrap_roles.sql
BEGIN;
SET LOCAL lock_timeout = '15s';
SET LOCAL statement_timeout = '60s';
SELECT pg_catalog.pg_advisory_xact_lock(6404202605);
SET LOCAL ROLE oo_target_owner;
\\i /migrations/0001_target_fence_expand.sql
SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;
\\i /migrations/0003_target_segment_preflight_expand.sql
\\i /migrations/0004_physical_mapping.sql
INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES
  ('target-bootstrap/1', '${BOOT_SHA}', 'expand', '0000_bootstrap_roles.sql'),
  ('target-fence-receipts/1', '${FENCE_SHA}', 'expand', '0001_target_fence_expand.sql'),
  ('target-fence-receipts/3', '${PRE_SHA}', 'expand', '0003_target_segment_preflight_expand.sql'),
  ('physical-mapping/v1', '${MAP_SHA}', 'expand', '0004_physical_mapping.sql');
COMMIT;
GRANT pg_read_all_stats TO oo_target_owner WITH INHERIT TRUE, SET FALSE;
BEGIN;
SET LOCAL lock_timeout = '15s';
SET LOCAL statement_timeout = '60s';
SELECT pg_catalog.pg_advisory_xact_lock(6404202605);
SET LOCAL ROLE oo_target_owner;
SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;
\\i /migrations/0005_full_import_expand.sql
INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES
  ('full-import/v1', '${FULL_SHA}', 'expand', '0005_full_import_expand.sql');
COMMIT;
SQL
if [ "$(cat "$OUT/install.exit")" != 0 ]; then
  echo "install failed" >&2
  printf '%s\n' 1 > "$PG/probe.exit"
  exit 1
fi

echo "=== fixture roles and owner table ==="
if ! grep -Eq '^[0-9a-f]{48}$' "$SEC/login.pass"; then
  echo "login password is not 48 hex" >&2
  printf '%s\n' 1 > "$PG/probe.exit"
  exit 1
fi
docker exec -u postgres -i "$NAME" bash -s <<'EOS' > "$OUT/fixture.stdout" 2> "$OUT/fixture.stderr"
set -u
export PGPASSWORD="$(cat /secrets/admin.pass)"
export PGSSLMODE=disable
pass=$(cat /secrets/login.pass)
psql -h 127.0.0.1 -U oo_fixture_admin -d oo_a169 -v ON_ERROR_STOP=1 -X -q <<SQL
CREATE ROLE oo_a169_login LOGIN PASSWORD '${pass}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION;
CREATE ROLE oo_r_a NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION;
CREATE ROLE oo_r_b NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION;
GRANT CONNECT ON DATABASE oo_a169 TO oo_a169_login;
GRANT oo_r_a TO oo_a169_login WITH INHERIT FALSE, SET TRUE;
GRANT oo_r_b TO oo_a169_login WITH INHERIT FALSE, SET TRUE;
GRANT EXECUTE ON FUNCTION pg_catalog.set_config(text, text, boolean) TO oo_a169_login;
SET ROLE oo_target_owner;
CREATE TABLE _oo.probe_orders (
  id integer NOT NULL,
  region text NOT NULL,
  secret text NOT NULL
);
INSERT INTO _oo.probe_orders (id, region, secret) VALUES
  (1, 'eu', 'hidden-eu'),
  (2, 'us', 'hidden-us');
RESET ROLE;
GRANT USAGE ON SCHEMA _oo TO oo_r_a;
GRANT USAGE ON SCHEMA _oo TO oo_r_b;
GRANT SELECT (id, region) ON TABLE _oo.probe_orders TO oo_r_a;
GRANT SELECT (id) ON TABLE _oo.probe_orders TO oo_r_b;
SQL
status=$?
unset PGPASSWORD
unset pass
exit "$status"
EOS
printf '%s\n' "$?" > "$OUT/fixture.exit"
scrub_out
if [ "$(cat "$OUT/fixture.exit")" != 0 ]; then
  echo "fixture failed" >&2
  printf '%s\n' 1 > "$PG/probe.exit"
  exit 1
fi

echo "=== tls identity ==="
run_login tls_identity "SELECT current_user, session_user, inet_server_addr() IS NOT NULL AS tcp, (SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()) AS ssl, version();"

echo "=== role a read ==="
run_login role_a_read "$(cat <<'SQL'
BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;
SELECT pg_backend_pid() AS pid, current_user AS before_role;
SET LOCAL ROLE oo_r_a;
SELECT current_user AS bound_role;
SELECT id, region FROM _oo.probe_orders ORDER BY id;
ROLLBACK;
SQL
)"

echo "=== hidden column ==="
run_login hidden_column "$(cat <<'SQL'
BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;
SET LOCAL ROLE oo_r_a;
SELECT secret FROM _oo.probe_orders;
ROLLBACK;
SQL
)"

echo "=== role b same session class ==="
run_login role_b_bleed "$(cat <<'SQL'
\set ON_ERROR_STOP off
SELECT pg_backend_pid() AS pid_before;
BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;
SET LOCAL ROLE oo_r_a;
SELECT count(*) AS a_rows FROM _oo.probe_orders;
ROLLBACK;
BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;
SET LOCAL ROLE oo_r_b;
SELECT id FROM _oo.probe_orders ORDER BY id;
SELECT region FROM _oo.probe_orders;
ROLLBACK;
SELECT pg_backend_pid() AS pid_after, current_user AS after_reset;
SQL
)"

echo "=== login without set role ==="
run_login login_direct "SELECT id FROM _oo.probe_orders;"

echo "=== write denial ==="
run_login write_denial "$(cat <<'SQL'
BEGIN;
SET LOCAL ROLE oo_r_a;
INSERT INTO _oo.probe_orders (id, region, secret) VALUES (9, 'eu', 'x');
ROLLBACK;
SQL
)"

echo "=== new column ==="
admin_sql add_column oo_a169 <<'SQL'
SET ROLE oo_target_owner;
ALTER TABLE _oo.probe_orders ADD COLUMN extra text;
RESET ROLE;
SELECT current_user;
SQL
run_login new_column "$(cat <<'SQL'
BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;
SET LOCAL ROLE oo_r_a;
SELECT extra FROM _oo.probe_orders;
ROLLBACK;
SQL
)"

echo "=== set_config and utility SET ==="
run_login set_config_denied "$(cat <<'SQL'
BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;
SELECT set_config('oo.claims', '{"region":["eu"]}', true);
SET LOCAL ROLE oo_r_a;
SELECT set_config('oo.claims', '{"region":["us"]}', true);
ROLLBACK;
SQL
)"
run_login utility_set "$(cat <<'SQL'
\set ON_ERROR_STOP off
BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;
SELECT set_config('oo.claims', '{"region":["eu"]}', true);
SET LOCAL ROLE oo_r_a;
SET LOCAL oo.claims = '{"region":["us"]}';
SELECT current_setting('oo.claims', true) AS claims_after_set;
ROLLBACK;
SQL
)"
run_login reset_role "$(cat <<'SQL'
BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;
SELECT set_config('oo.claims', '{"region":["eu"]}', true);
SET LOCAL ROLE oo_r_a;
RESET ROLE;
SELECT current_user AS after_reset, set_config('oo.claims', '{"region":["us"]}', true) AS reset_set;
SET LOCAL ROLE oo_r_a;
SELECT current_setting('oo.claims', true) AS claims_after_rebind;
ROLLBACK;
SQL
)"

echo "=== audit ==="
admin_sql audit oo_a169 <<'SQL'
\set ON_ERROR_STOP on
SELECT rolname, rolsuper, rolcreaterole, rolcreatedb, rolcanlogin, rolinherit, rolbypassrls, rolreplication
FROM pg_roles
WHERE rolname IN ('oo_fixture_admin', 'oo_target_owner', 'oo_pipeline_runtime', 'oo_a169_login', 'oo_r_a', 'oo_r_b')
ORDER BY rolname;
SELECT tableowner.relowner::regrole AS owner, current_user AS installer
FROM pg_class AS tableowner
JOIN pg_namespace ON pg_namespace.oid = tableowner.relnamespace
WHERE pg_namespace.nspname = '_oo' AND tableowner.relname = 'probe_orders';
SELECT grantee, privilege_type, column_name
FROM information_schema.column_privileges
WHERE table_schema = '_oo' AND table_name = 'probe_orders'
ORDER BY grantee, column_name, privilege_type;
SELECT grantee, privilege_type
FROM information_schema.table_privileges
WHERE table_schema = '_oo' AND table_name = 'probe_orders'
ORDER BY grantee, privilege_type;
SELECT member.rolname AS member, granted.rolname AS granted, grantor.rolname AS grantor,
       auth.admin_option, auth.inherit_option, auth.set_option
FROM pg_auth_members AS auth
JOIN pg_roles AS member ON member.oid = auth.member
JOIN pg_roles AS granted ON granted.oid = auth.roleid
JOIN pg_roles AS grantor ON grantor.oid = auth.grantor
WHERE member.rolname IN ('oo_a169_login', 'oo_r_a', 'oo_r_b', 'oo_target_owner')
   OR granted.rolname IN ('oo_r_a', 'oo_r_b', 'oo_target_owner')
ORDER BY member.rolname, granted.rolname;
SELECT has_table_privilege('oo_r_a', '_oo.probe_orders', 'SELECT') AS table_select,
       has_table_privilege('oo_r_a', '_oo.probe_orders', 'INSERT') AS table_insert,
       has_column_privilege('oo_r_a', '_oo.probe_orders', 'secret', 'SELECT') AS secret_select,
       has_column_privilege('oo_r_a', '_oo.probe_orders', 'extra', 'SELECT') AS extra_select,
       has_column_privilege('oo_r_a', '_oo.probe_orders', 'id', 'SELECT') AS id_select,
       has_table_privilege('oo_r_a', '_oo.physical_mappings', 'SELECT') AS mapping_select,
       has_table_privilege('oo_r_a', '_oo.full_import_run', 'SELECT') AS import_select;
SQL

echo "=== direct metadata ==="
run_login metadata_denied "SELECT count(*) FROM _oo.physical_mappings;"

echo "=== redact check ==="
if grep -R -F -f "$SEC/admin.pass" "$OUT" >/dev/null 2>&1 || grep -R -F -f "$SEC/login.pass" "$OUT" >/dev/null 2>&1; then
  echo "secret leaked into probe output" >&2
  printf '%s\n' 1 > "$PG/probe.exit"
  exit 1
fi

printf '%s\n' 0 > "$PG/probe.exit"
echo "probe_finished"
