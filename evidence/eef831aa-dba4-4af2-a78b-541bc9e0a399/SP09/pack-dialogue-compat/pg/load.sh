#!/bin/bash
# Apply the reviewed migration sequence and one 90000-row fixture. Passwords stay in files.
set -eu
hex48() {
  value=$(tr -d '\n' < "$1")
  case "$value" in
    *[!0-9a-fA-F]*|"") echo "password file is not hex" >&2; exit 1 ;;
  esac
  if [ "${#value}" -ne 48 ]; then
    echo "password file is not 48 hex characters" >&2
    exit 1
  fi
  printf '%s' "$value"
}
admin=$(hex48 /secrets/admin.pass)
login=$(hex48 /secrets/login.pass)
export PGPASSWORD="$admin"
sha() { sha256sum "$1" | awk '{print $1}'; }
b0=$(sha /migrations/0000_bootstrap_roles.sql)
b1=$(sha /migrations/0001_target_fence_expand.sql)
b3=$(sha /migrations/0003_target_segment_preflight_expand.sql)
b4=$(sha /migrations/0004_physical_mapping.sql)
b5=$(sha /migrations/0005_full_import_expand.sql)
psql -v ON_ERROR_STOP=1 -U oo_fixture_admin -d oo_a399 \
  -c "CREATE ROLE oo_a399_login LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION PASSWORD '${login}'" \
  -c "GRANT CONNECT ON DATABASE oo_a399 TO oo_a399_login" >/dev/null
unset login
psql -v ON_ERROR_STOP=1 -U oo_fixture_admin -d oo_a399 <<SQL
BEGIN;
SET LOCAL lock_timeout = '15s';
SET LOCAL statement_timeout = '120s';
SELECT pg_catalog.pg_advisory_xact_lock(64042026399);
\\i /migrations/0000_bootstrap_roles.sql
SET LOCAL ROLE oo_target_owner;
\\i /migrations/0001_target_fence_expand.sql
SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;
\\i /migrations/0003_target_segment_preflight_expand.sql
\\i /migrations/0004_physical_mapping.sql
INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES
  ('target-bootstrap/1', '${b0}', 'expand', '0000_bootstrap_roles.sql'),
  ('target-fence-receipts/1', '${b1}', 'expand', '0001_target_fence_expand.sql'),
  ('target-fence-receipts/3', '${b3}', 'expand', '0003_target_segment_preflight_expand.sql'),
  ('physical-mapping/v1', '${b4}', 'expand', '0004_physical_mapping.sql');
COMMIT;
BEGIN;
SET LOCAL lock_timeout = '15s';
SET LOCAL statement_timeout = '120s';
SELECT pg_catalog.pg_advisory_xact_lock(64042026399);
SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;
GRANT pg_read_all_stats TO oo_target_owner WITH INHERIT TRUE, SET FALSE;
SET LOCAL ROLE oo_target_owner;
\\i /migrations/0005_full_import_expand.sql
INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES
  ('full-import/v1', '${b5}', 'expand', '0005_full_import_expand.sql');
COMMIT;
SQL
psql -v ON_ERROR_STOP=1 -U oo_fixture_admin -d oo_a399 <<'SQL'
SELECT _oo.adopt_target_generation('shard_a399', 'a3990000-0000-4000-8000-000000000399'::uuid) AS target_generation \gset
SELECT _oo.full_import_bind_session('shard_a399', 'a3990000-0000-4000-8000-000000000399'::uuid, :target_generation);
SELECT _oo.register_physical_mapping('shop', 'e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e', 1, 'namespace', 'public', '', '', _oo.pm_sha256(_oo.pm_payload('shop', 'e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e', 1, 'n', 'public', '', '')));
SELECT _oo.register_physical_mapping('shop', 'e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e', 1, 'table', 'public', 'entry', '', _oo.pm_sha256(_oo.pm_payload('shop', 'e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e', 1, 't', 'public', 'entry', '')));
SELECT _oo.register_physical_mapping('shop', 'e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e', 1, 'column', 'public', 'entry', 'entry_count', _oo.pm_sha256(_oo.pm_payload('shop', 'e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e', 1, 'c', 'public', 'entry', 'entry_count')));
SELECT _oo.lookup_physical_mapping('shop', 'e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e', 1, 'table', 'public', 'entry', '') AS physical_table \gset
SELECT _oo.lookup_physical_mapping('shop', 'e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e', 1, 'column', 'public', 'entry', 'entry_count') AS physical_column \gset
SELECT t.oid::int8 AS int4_oid FROM pg_catalog.pg_type AS t JOIN pg_catalog.pg_namespace AS n ON n.oid = t.typnamespace WHERE n.nspname = 'pg_catalog' AND t.typname = 'int4' AND t.typtype = 'b' \gset
SELECT _oo.full_import_open_run('shard_a399', 'a3990000-0000-4000-8000-000000000399'::uuid, :target_generation, 'shop', 1, 'e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e', 'b399b399b399b399b399b399b399b399b399b399b399b399b399b399b399b399', 'c399c399c399c399c399c399c399c399c399c399c399c399c399c399c399c399', 'd399d399d399d399d399d399d399d399d399d399d399d399d399d399d399d399', ARRAY['public'], ARRAY['entry'], ARRAY[:'physical_table']) AS run_id \gset
SELECT _oo.full_import_prepare_shadow('shard_a399', 'a3990000-0000-4000-8000-000000000399'::uuid, :target_generation, :'run_id'::uuid, 0, 'shop', 'e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e', 1, 'public', 'entry', ARRAY['entry_count'], ARRAY[:int4_oid]::bigint[], ARRAY[-1]::integer[], ARRAY[true]::boolean[]);
SELECT set_config('statement_timeout', '180s', false);
SELECT format('INSERT INTO _oo.%I ("_oo_pos", %I) SELECT g::numeric, g::integer FROM generate_series(1, 90000) AS g;', :'physical_table', :'physical_column') AS insert_sql \gset
:insert_sql
SELECT format('ANALYZE _oo.%I;', :'physical_table') AS analyze_sql \gset
:analyze_sql
SELECT format('SELECT count(*) FROM _oo.%I;', :'physical_table') AS count_sql \gset
:count_sql
SQL
unset PGPASSWORD
unset admin
echo "fixture_load_done"
