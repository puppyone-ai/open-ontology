#!/bin/bash
# Inside the fixture as the postgres OS user. Peer auth. No argv secrets.
# Roles are created before log_statement=all. The audit marker is written only
# after the live rows and catalog match the preregistered files.
set -eu
export PGHOST=/var/run/postgresql
export PGUSER=postgres
export PGDATABASE=lotdb

ROWS_SHA=99ff6b0be78bbf745cc1ce2d55efe05b9a39c92bc4c6f6c0d79dcd638c857e17
CATALOG_SHA=eb9a85123f3e9f4611c058bf0833cc4fb0bfebee39484ee3ac0cbc34becd44e9
WRITER_SHA=0eab79c34d8c9d14fe8f2157ac651f5967d205cd3bd205db7290fe06d551d79f

fail() {
  echo "init_fail=$1" >&2
  exit 1
}

reader=$(cat /secrets/reader.pass)
writer=$(cat /secrets/writer.pass)
have_reader=$(psql -At -c "SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'lot_reader'")
if [ "$have_reader" != "1" ]; then
  umask 077
  cat > /tmp/a121-roles.sql <<EOF
CREATE ROLE lot_reader LOGIN NOSUPERUSER NOBYPASSRLS NOINHERIT NOCREATEDB NOCREATEROLE PASSWORD '${reader}';
CREATE ROLE lot_writer LOGIN NOSUPERUSER NOBYPASSRLS NOINHERIT NOCREATEDB NOCREATEROLE PASSWORD '${writer}';
EOF
  chmod 600 /tmp/a121-roles.sql
  psql -v ON_ERROR_STOP=1 -f /tmp/a121-roles.sql
  psql -v ON_ERROR_STOP=1 -f /fixture/init.sql
  rm -f /tmp/a121-roles.sql
fi
unset reader writer

psql -v ON_ERROR_STOP=1 -c "ALTER SYSTEM SET log_statement = 'all'"
psql -v ON_ERROR_STOP=1 -c "SELECT pg_catalog.pg_reload_conf()"
sleep 1

got=$(sha256sum /expected/rows.txt | awk '{print $1}')
if [ "$got" != "$ROWS_SHA" ]; then
  fail rows_hash
fi
got=$(sha256sum /expected/catalog.txt | awk '{print $1}')
if [ "$got" != "$CATALOG_SHA" ]; then
  fail catalog_hash
fi
got=$(sha256sum /expected/writer.sql | awk '{print $1}')
if [ "$got" != "$WRITER_SHA" ]; then
  fail writer_hash
fi

psql -v ON_ERROR_STOP=1 -At -F "$(printf '\t')" >/tmp/a121-got-catalog.txt <<'SQL'
SELECT m.phase, n.nspname, c.relname, a.attnum::text, a.attname, a.atttypid::text, a.atttypmod::text,
       CASE WHEN a.attnotnull THEN 'true' ELSE 'false' END,
       COALESCE((
           SELECT k.ord::text
           FROM (
               SELECT a2.attname, u.ord
               FROM pg_catalog.pg_constraint con
               JOIN LATERAL unnest(con.conkey) WITH ORDINALITY AS u(attnum, ord) ON true
               JOIN pg_catalog.pg_attribute a2 ON a2.attrelid = con.conrelid AND a2.attnum = u.attnum
               WHERE con.conrelid = c.oid AND con.contype = 'p'
           ) k
           WHERE k.attname = a.attname
       ), '0')
FROM pg_catalog.pg_attribute a
JOIN pg_catalog.pg_class c ON c.oid = a.attrelid
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
JOIN (VALUES
    ('cut', 'lot', 'batch'),
    ('cut', 'lot', 'child row'),
    ('sheet', 'acl', 'sheet'),
    ('wide', 'acl', 'wide'),
    ('drift', 'drift', 'item'),
    ('hold', 'life', 'hold')
) AS m(phase, schema_name, relname) ON m.schema_name = n.nspname AND m.relname = c.relname
WHERE a.attnum > 0 AND NOT a.attisdropped
ORDER BY 1, 2, 3, 4;
SQL
grep -v '^drift_new	' /expected/catalog.txt | sort > /tmp/a121-want-catalog.txt
sort /tmp/a121-got-catalog.txt > /tmp/a121-got-catalog-sorted.txt
if ! cmp -s /tmp/a121-want-catalog.txt /tmp/a121-got-catalog-sorted.txt; then
  diff -u /tmp/a121-want-catalog.txt /tmp/a121-got-catalog-sorted.txt >&2 || true
  fail catalog_rows
fi

psql -v ON_ERROR_STOP=1 -At -F "$(printf '\t')" >/tmp/a121-got-rows.txt <<'SQL'
SELECT 'cut', 'batch', id::text,
       'int:' || id::text || ' text:' || title || ' bool:' || CASE WHEN ready THEN 'true' ELSE 'false' END
       || ' text:' || code || ' ' || CASE WHEN "odd""mark" IS NULL THEN 'null' ELSE 'text:' || "odd""mark" END
FROM lot.batch
UNION ALL
SELECT 'cut', 'child row', id::text,
       'int:' || id::text || ' int:' || batch_id::text || ' int:' || qty::text || ' '
       || CASE WHEN note IS NULL THEN 'null' ELSE 'text:' || note END
FROM lot."child row"
UNION ALL
SELECT 'sheet', 'sheet', id::text,
       'int:' || id::text || ' text:' || tag || ' ' || CASE WHEN note IS NULL THEN 'null' ELSE 'text:' || note END
FROM acl.sheet
UNION ALL
SELECT 'wide', 'wide', id::text,
       'int:' || id::text || ' text:' || tag || ' ' || CASE WHEN note IS NULL THEN 'null' ELSE 'text:' || note END
FROM acl.wide
UNION ALL
SELECT 'drift', 'item', id::text, 'int:' || id::text || ' text:' || label
FROM drift.item
UNION ALL
SELECT 'hold', 'hold', id::text, 'int:' || id::text || ' text:' || label
FROM life.hold
ORDER BY 1, 2, 3;
SQL
awk -F '\t' '$1 ~ /^(cut|sheet|wide|drift|hold)$/ && $4 != "ABSENT"' /expected/rows.txt | sort > /tmp/a121-want-rows.txt
sort /tmp/a121-got-rows.txt > /tmp/a121-got-rows-sorted.txt
if ! cmp -s /tmp/a121-want-rows.txt /tmp/a121-got-rows-sorted.txt; then
  diff -u /tmp/a121-want-rows.txt /tmp/a121-got-rows-sorted.txt >&2 || true
  fail data_rows
fi

absent=$(psql -At -c "SELECT count(*) FROM lot.\"child row\" WHERE id IN (44, 90)")
if [ "$absent" != "0" ]; then
  fail absent_child
fi
canary=$(psql -At -c "SELECT hits FROM lot.tick WHERE n = 1")
if [ "$canary" != "0" ]; then
  fail canary
fi
secdef=$(psql -At -c "SELECT prosecdef FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace WHERE n.nspname = 'lot' AND p.proname = 'bump'")
if [ "$secdef" != "f" ]; then
  fail bump_secdef
fi
sheet_table=$(psql -At -c "SELECT has_table_privilege('lot_reader', 'acl.sheet', 'SELECT')")
sheet_note=$(psql -At -c "SELECT has_column_privilege('lot_reader', 'acl.sheet', 'note', 'SELECT')")
wide_table=$(psql -At -c "SELECT has_table_privilege('lot_reader', 'acl.wide', 'SELECT')")
reader_insert=$(psql -At -c "SELECT has_table_privilege('lot_reader', 'lot.batch', 'INSERT')")
reader_select=$(psql -At -c "SELECT has_table_privilege('lot_reader', 'lot.batch', 'SELECT')")
members=$(psql -At -c "SELECT count(*) FROM pg_catalog.pg_auth_members m JOIN pg_catalog.pg_roles r ON r.oid = m.member WHERE r.rolname = 'lot_reader'")
super=$(psql -At -c "SELECT CASE WHEN rolsuper THEN 't' ELSE 'f' END || CASE WHEN rolbypassrls THEN 't' ELSE 'f' END FROM pg_catalog.pg_roles WHERE rolname = 'lot_reader'")
owner=$(psql -At -c "SELECT r.rolname FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace JOIN pg_catalog.pg_roles r ON r.oid = c.relowner WHERE n.nspname = 'acl' AND c.relname = 'sheet'")
if [ "$sheet_table" != "f" ] || [ "$sheet_note" != "t" ] || [ "$wide_table" != "t" ]; then
  fail column_grants
fi
if [ "$reader_insert" != "f" ] || [ "$reader_select" != "t" ] || [ "$members" != "0" ] || [ "$super" != "ff" ] || [ "$owner" = "lot_reader" ]; then
  fail reader_boundary
fi
version=$(psql -At -c "SHOW server_version")
case "$version" in
  17.10*) ;;
  *) fail server_version ;;
esac
recovery=$(psql -At -c "SELECT pg_catalog.pg_is_in_recovery()")
if [ "$recovery" != "f" ]; then
  fail recovery
fi

psql -v ON_ERROR_STOP=1 -c "SELECT 'A121_AUDIT_WINDOW_START'" >/dev/null
sleep 1
if ! grep -F -q "A121_AUDIT_WINDOW_START" /pglog/postgresql.log; then
  fail audit_marker_missing
fi
if [ ! -s /pglog/postgresql.log ]; then
  fail audit_empty
fi
leak=0
for file in /secrets/reader.pass /secrets/writer.pass /secrets/super.pass; do
  secret=$(cat "$file")
  if [ -n "$secret" ] && grep -F -q -- "$secret" /pglog/postgresql.log; then
    leak=1
  fi
done
if [ "$leak" -ne 0 ]; then
  fail log_leak
fi
echo "init_ok"
echo "log_leak=no"
echo "server_version=$version"
