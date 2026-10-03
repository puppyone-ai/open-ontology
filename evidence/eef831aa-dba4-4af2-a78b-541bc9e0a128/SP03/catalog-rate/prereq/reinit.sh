#!/bin/bash
# Restore the snap fixture after snapshot_live mutated it. Roles already exist.
# Does not recreate roles and does not read password files.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a128
PG=${RUN_ID}-pg
docker exec -u postgres "$PG" psql -d snapdb -v ON_ERROR_STOP=1 -c \
  "DROP SCHEMA IF EXISTS snap CASCADE; DROP SCHEMA IF EXISTS lot CASCADE; DROP SCHEMA IF EXISTS acl CASCADE; DROP SCHEMA IF EXISTS drift CASCADE; DROP SCHEMA IF EXISTS life CASCADE; DROP SCHEMA IF EXISTS rate CASCADE;"
docker exec -u postgres "$PG" psql -d snapdb -v ON_ERROR_STOP=1 -f /fixture/init.sql
docker exec -u postgres "$PG" psql -d snapdb -v ON_ERROR_STOP=1 -At -c \
  "SELECT n.nspname || '.' || c.relname || ':' || string_agg(a.attname, ',' ORDER BY a.attnum) FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped WHERE n.nspname = 'snap' AND c.relname IN ('parent', 'child') GROUP BY n.nspname, c.relname ORDER BY c.relname;"
echo "reinit_ok"
