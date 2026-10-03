#!/bin/bash
# Inside the fixture as the postgres OS user. Peer auth. No argv secrets.
# Every role is created before log_statement=all.
set -eu
export PGHOST=/var/run/postgresql
export PGUSER=postgres
export PGDATABASE=snapdb

reader=$(cat /secrets/reader.pass)
writer=$(cat /secrets/writer.pass)
guc=$(cat /secrets/guc.pass)
lot_reader=$(cat /secrets/lot-reader.pass)
lot_writer=$(cat /secrets/lot-writer.pass)
umask 077
cat > /tmp/a130-roles.sql <<EOF
CREATE ROLE oo_snap_reader LOGIN PASSWORD '${reader}';
CREATE ROLE oo_snap_writer LOGIN PASSWORD '${writer}';
CREATE ROLE oo_snap_guc LOGIN PASSWORD '${guc}';
CREATE ROLE lot_reader LOGIN NOSUPERUSER NOBYPASSRLS NOINHERIT NOCREATEDB NOCREATEROLE PASSWORD '${lot_reader}';
CREATE ROLE lot_writer LOGIN NOSUPERUSER NOBYPASSRLS NOINHERIT NOCREATEDB NOCREATEROLE PASSWORD '${lot_writer}';
CREATE ROLE oo_a127_reader LOGIN PASSWORD '${reader}';
CREATE ROLE oo_a127_writer LOGIN PASSWORD '${writer}';
EOF
chmod 600 /tmp/a130-roles.sql
psql -v ON_ERROR_STOP=1 -f /tmp/a130-roles.sql
psql -v ON_ERROR_STOP=1 -f /fixture/init.sql
psql -v ON_ERROR_STOP=1 -f /fixture/portal-init.sql
rm -f /tmp/a130-roles.sql
unset reader writer guc lot_reader lot_writer

psql -v ON_ERROR_STOP=1 -c "ALTER SYSTEM SET log_statement = 'all'"
psql -v ON_ERROR_STOP=1 -c "SELECT pg_catalog.pg_reload_conf()"
: > /pglog/postgresql.log
sleep 1
: > /pglog/postgresql.log
leak=0
for file in /secrets/reader.pass /secrets/writer.pass /secrets/guc.pass /secrets/super.pass /secrets/lot-reader.pass /secrets/lot-writer.pass; do
  if [ -s "$file" ] && grep -F -q -f "$file" /pglog/postgresql.log; then
    leak=1
  fi
done
if [ "$leak" -ne 0 ]; then
  echo "log_leak=yes" >&2
  exit 1
fi
echo "log_leak=no"

version=$(psql -At -c "SHOW server_version")
case "$version" in
  17.10*) ;;
  *) echo "server_version=$version" >&2; exit 1 ;;
esac
echo "server_version=$version"

parent=$(psql -At -c "SELECT label FROM snap.parent WHERE id = 1")
if [ "$parent" != "alpha" ]; then
  echo "snap seed mismatch" >&2
  exit 1
fi
rows=$(psql -At -c "SELECT id::text || ':' || COALESCE(note, 'NULL') FROM oo_a127.cut ORDER BY id")
want=$'1:one\n2:NULL\n3:three-original'
if [ "$rows" != "$want" ]; then
  echo "portal seed mismatch" >&2
  exit 1
fi
reader_insert=$(psql -At -c "SELECT has_table_privilege('oo_a127_reader', 'oo_a127.cut', 'INSERT')")
reader_select=$(psql -At -c "SELECT has_table_privilege('oo_a127_reader', 'oo_a127.cut', 'SELECT')")
writer_insert=$(psql -At -c "SELECT has_table_privilege('oo_a127_writer', 'oo_a127.cut', 'INSERT')")
writer_update=$(psql -At -c "SELECT has_table_privilege('oo_a127_writer', 'oo_a127.cut', 'UPDATE')")
snap_select=$(psql -At -c "SELECT has_table_privilege('oo_snap_reader', 'snap.parent', 'SELECT')")
snap_insert=$(psql -At -c "SELECT has_table_privilege('oo_snap_reader', 'snap.parent', 'INSERT')")
if [ "$reader_insert" != "f" ] || [ "$reader_select" != "t" ] || [ "$writer_insert" != "t" ] || [ "$writer_update" != "t" ]; then
  echo "portal privilege mismatch" >&2
  exit 1
fi
if [ "$snap_select" != "t" ] || [ "$snap_insert" != "f" ]; then
  echo "snap privilege mismatch" >&2
  exit 1
fi
echo "init_ok version=$version"
