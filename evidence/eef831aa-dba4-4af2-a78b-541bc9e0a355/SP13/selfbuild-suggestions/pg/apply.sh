#!/bin/bash
# One session installs the frozen migrations, then one session binds the shop.account.party_key shadow.
# Passwords are read from files. They are not printed.
set -euo pipefail
export PGPASSWORD
PGPASSWORD=$(cat /secrets/admin.pass)
PSQL=(psql -v ON_ERROR_STOP=1 -h 127.0.0.1 -U oo_a355_admin -d oo_a355)
sha() { sha256sum "$1" | awk '{print $1}'; }
b0=$(sha /migrations/0000_bootstrap_roles.sql)
b1=$(sha /migrations/0001_target_fence_expand.sql)
b3=$(sha /migrations/0003_target_segment_preflight_expand.sql)
b4=$(sha /migrations/0004_physical_mapping.sql)
b5=$(sha /migrations/0005_full_import_expand.sql)
"${PSQL[@]}" <<SQL
BEGIN;
SET LOCAL lock_timeout = '15s';
SET LOCAL statement_timeout = '120s';
SELECT pg_catalog.pg_advisory_xact_lock(64042026355);
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
SELECT pg_catalog.pg_advisory_xact_lock(64042026355);
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
CREATE ROLE oo_a355_login LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION PASSWORD '${login_pass}';
GRANT CONNECT ON DATABASE oo_a355 TO oo_a355_login;
GRANT oo_r_reviewer TO oo_a355_login;
GRANT USAGE ON SCHEMA _oo TO oo_a355_login;
GRANT SELECT ON TABLE _oo.physical_mappings TO oo_a355_login;
GRANT EXECUTE ON FUNCTION _oo.lookup_physical_mapping(text, text, bigint, text, text, text, text) TO oo_a355_login;
SQL
unset login_pass
"${PSQL[@]}" <<'SQL'
SELECT _oo.adopt_target_generation('shard_a355', 'a3550000-0000-4000-8000-000000000355'::uuid) AS generation \gset
SELECT _oo.full_import_bind_session('shard_a355', 'a3550000-0000-4000-8000-000000000355'::uuid, :generation);
SELECT _oo.register_physical_mapping(
  'pubfix',
  'a355a355a355a355a355a355a355a355a355a355a355a355a355a355a355a355',
  355,
  'namespace',
  'shop',
  '',
  '',
  _oo.pm_sha256(_oo.pm_payload(
    'pubfix',
    'a355a355a355a355a355a355a355a355a355a355a355a355a355a355a355a355',
    355, 'n', 'shop', '', ''))
);
SELECT _oo.register_physical_mapping(
  'pubfix',
  'a355a355a355a355a355a355a355a355a355a355a355a355a355a355a355a355',
  355,
  'table',
  'shop',
  'account',
  '',
  _oo.pm_sha256(_oo.pm_payload(
    'pubfix',
    'a355a355a355a355a355a355a355a355a355a355a355a355a355a355a355a355',
    355, 't', 'shop', 'account', ''))
);
SELECT _oo.register_physical_mapping(
  'pubfix',
  'a355a355a355a355a355a355a355a355a355a355a355a355a355a355a355a355',
  355,
  'column',
  'shop',
  'account',
  'party_key',
  _oo.pm_sha256(_oo.pm_payload(
    'pubfix',
    'a355a355a355a355a355a355a355a355a355a355a355a355a355a355a355a355',
    355, 'c', 'shop', 'account', 'party_key'))
);
SELECT _oo.lookup_physical_mapping(
  'pubfix',
  'a355a355a355a355a355a355a355a355a355a355a355a355a355a355a355a355',
  355, 'table', 'shop', 'account', '') AS physical_table \gset
SELECT _oo.lookup_physical_mapping(
  'pubfix',
  'a355a355a355a355a355a355a355a355a355a355a355a355a355a355a355a355',
  355, 'column', 'shop', 'account', 'party_key') AS physical_column \gset
SELECT type_row.oid::bigint AS text_oid
  FROM pg_catalog.pg_type AS type_row
  JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = type_row.typnamespace
  WHERE namespace.nspname = 'pg_catalog' AND type_row.typname = 'text' AND type_row.typtype = 'b' \gset
SELECT _oo.full_import_open_run(
  'shard_a355',
  'a3550000-0000-4000-8000-000000000355'::uuid,
  :generation,
  'pubfix',
  355,
  'a355a355a355a355a355a355a355a355a355a355a355a355a355a355a355a355',
  'b355b355b355b355b355b355b355b355b355b355b355b355b355b355b355b355',
  'c355c355c355c355c355c355c355c355c355c355c355c355c355c355c355c355',
  'd355d355d355d355d355d355d355d355d355d355d355d355d355d355d355d355',
  ARRAY['shop'],
  ARRAY['account'],
  ARRAY[:'physical_table']
)::text AS run_id \gset
SELECT _oo.full_import_prepare_shadow(
  'shard_a355',
  'a3550000-0000-4000-8000-000000000355'::uuid,
  :generation,
  :'run_id'::uuid,
  0,
  'pubfix',
  'a355a355a355a355a355a355a355a355a355a355a355a355a355a355a355a355',
  355,
  'shop',
  'account',
  ARRAY['party_key'],
  ARRAY[:text_oid]::bigint[],
  ARRAY[-1]::integer[],
  ARRAY[true]
);
INSERT INTO _oo.:"physical_table" ("_oo_pos", :"physical_column") VALUES (1, 'party-1');
ANALYZE _oo.:"physical_table";
GRANT USAGE ON SCHEMA _oo TO oo_r_reviewer;
GRANT SELECT (:"physical_column") ON TABLE _oo.:"physical_table" TO oo_r_reviewer;
SQL
echo "shadow ready"
