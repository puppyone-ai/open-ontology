#!/bin/bash
# Install the frozen migrations, then bind one shop.account.party_key shadow row.
# The row is the literal a364-mapped-row. This is not a 100k load.
# Passwords are read from files. They are not printed.
set -euo pipefail
export PGPASSWORD
PGPASSWORD=$(cat /secrets/admin.pass)
PSQL=(psql -v ON_ERROR_STOP=1 -h 127.0.0.1 -U oo_a364_admin -d oo_a364)
sha() { sha256sum "$1" | awk '{print $1}'; }
b0=$(sha /migrations/0000_bootstrap_roles.sql)
b1=$(sha /migrations/0001_target_fence_expand.sql)
b3=$(sha /migrations/0003_target_segment_preflight_expand.sql)
b4=$(sha /migrations/0004_physical_mapping.sql)
b5=$(sha /migrations/0005_full_import_expand.sql)
DIGEST=a364a364a364a364a364a364a364a364a364a364a364a364a364a364a364a364
BHASH=b364b364b364b364b364b364b364b364b364b364b364b364b364b364b364b364
CHASH=c364c364c364c364c364c364c364c364c364c364c364c364c364c364c364c364
DHASH=d364d364d364d364d364d364d364d364d364d364d364d364d364d364d364d364
test "${#DIGEST}" -eq 64
test "${#BHASH}" -eq 64
test "${#CHASH}" -eq 64
test "${#DHASH}" -eq 64
UUID=a3640000-0000-4000-8000-000000000364
"${PSQL[@]}" <<SQL
BEGIN;
SET LOCAL lock_timeout = '15s';
SET LOCAL statement_timeout = '120s';
SELECT pg_catalog.pg_advisory_xact_lock(64042026364);
\i /migrations/0000_bootstrap_roles.sql
SET LOCAL ROLE oo_target_owner;
\i /migrations/0001_target_fence_expand.sql
SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;
\i /migrations/0003_target_segment_preflight_expand.sql
\i /migrations/0004_physical_mapping.sql
INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES
  ('target-bootstrap/1', '${b0}', 'expand', '0000_bootstrap_roles.sql'),
  ('target-fence-receipts/1', '${b1}', 'expand', '0001_target_fence_expand.sql'),
  ('target-fence-receipts/3', '${b3}', 'expand', '0003_target_segment_preflight_expand.sql'),
  ('physical-mapping/v1', '${b4}', 'expand', '0004_physical_mapping.sql');
COMMIT;
SQL
"${PSQL[@]}" <<SQL
BEGIN;
SET LOCAL lock_timeout = '15s';
SET LOCAL statement_timeout = '120s';
SELECT pg_catalog.pg_advisory_xact_lock(64042026364);
SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;
GRANT pg_read_all_stats TO oo_target_owner WITH INHERIT TRUE, SET FALSE;
SET LOCAL ROLE oo_target_owner;
\i /migrations/0005_full_import_expand.sql
INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES
  ('full-import/v1', '${b5}', 'expand', '0005_full_import_expand.sql');
COMMIT;
SQL
login_pass=$(tr -d '\n' < /secrets/login.pass)
"${PSQL[@]}" <<SQL
CREATE ROLE oo_r_reviewer NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION;
CREATE ROLE oo_a364_login LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION PASSWORD '${login_pass}';
GRANT CONNECT ON DATABASE oo_a364 TO oo_a364_login;
GRANT oo_r_reviewer TO oo_a364_login;
GRANT USAGE ON SCHEMA _oo TO oo_a364_login;
GRANT SELECT ON TABLE _oo.physical_mappings TO oo_a364_login;
GRANT EXECUTE ON FUNCTION _oo.lookup_physical_mapping(text, text, bigint, text, text, text, text) TO oo_a364_login;
SQL
unset login_pass
unset PGPASSWORD
export PGPASSWORD
PGPASSWORD=$(cat /secrets/admin.pass)
"${PSQL[@]}" <<SQL
SELECT _oo.adopt_target_generation('shard_a364', '${UUID}'::uuid) AS generation \gset
SELECT _oo.full_import_bind_session('shard_a364', '${UUID}'::uuid, :generation);
SELECT _oo.register_physical_mapping(
  'a364',
  '${DIGEST}',
  364,
  'namespace',
  'shop',
  '',
  '',
  _oo.pm_sha256(_oo.pm_payload('a364', '${DIGEST}', 364, 'n', 'shop', '', ''))
);
SELECT _oo.register_physical_mapping(
  'a364',
  '${DIGEST}',
  364,
  'table',
  'shop',
  'account',
  '',
  _oo.pm_sha256(_oo.pm_payload('a364', '${DIGEST}', 364, 't', 'shop', 'account', ''))
);
SELECT _oo.register_physical_mapping(
  'a364',
  '${DIGEST}',
  364,
  'column',
  'shop',
  'account',
  'party_key',
  _oo.pm_sha256(_oo.pm_payload('a364', '${DIGEST}', 364, 'c', 'shop', 'account', 'party_key'))
);
SELECT _oo.lookup_physical_mapping(
  'a364', '${DIGEST}', 364, 'table', 'shop', 'account', '') AS physical_table \gset
SELECT _oo.lookup_physical_mapping(
  'a364', '${DIGEST}', 364, 'column', 'shop', 'account', 'party_key') AS physical_column \gset
SELECT type_row.oid::bigint AS text_oid
  FROM pg_catalog.pg_type AS type_row
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = type_row.typnamespace
  WHERE namespace.nspname = 'pg_catalog' AND type_row.typname = 'text' AND type_row.typtype = 'b' \gset
SELECT _oo.full_import_open_run(
  'shard_a364',
  '${UUID}'::uuid,
  :generation,
  'a364',
  364,
  '${DIGEST}',
  '${BHASH}',
  '${CHASH}',
  '${DHASH}',
  ARRAY['shop'],
  ARRAY['account'],
  ARRAY[:'physical_table']
)::text AS run_id \gset
SELECT _oo.full_import_prepare_shadow(
  'shard_a364',
  '${UUID}'::uuid,
  :generation,
  :'run_id'::uuid,
  0,
  'a364',
  '${DIGEST}',
  364,
  'shop',
  'account',
  ARRAY['party_key'],
  ARRAY[:text_oid]::bigint[],
  ARRAY[-1]::integer[],
  ARRAY[true]
);
INSERT INTO _oo.:"physical_table" ("_oo_pos", :"physical_column") VALUES (1, 'a364-mapped-row');
ANALYZE _oo.:"physical_table";
GRANT USAGE ON SCHEMA _oo TO oo_r_reviewer;
GRANT SELECT (:"physical_column") ON TABLE _oo.:"physical_table" TO oo_r_reviewer;
\pset format unaligned
\pset tuples_only on
SELECT 'mapped_rows=' || count(*)::text FROM _oo.:"physical_table";
SELECT 'mapped_value=' || :"physical_column"::text FROM _oo.:"physical_table";
SELECT 'server_version=' || current_setting('server_version');
SQL
echo "shadow ready"
echo "loaded_rows=1"
echo "not_100k_load=true"
