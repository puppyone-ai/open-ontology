#!/bin/bash
# Peer auth inside the fixture. Role passwords are read from files, not argv.
set -eu
export PGHOST=/var/run/postgresql
export PGUSER=postgres
export PGDATABASE=oo_a127
reader=$(cat /secrets/reader.pass)
writer=$(cat /secrets/writer.pass)
have_reader=$(psql -At -c "SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_a127_reader'")
if [ "$have_reader" != "1" ]; then
  umask 077
  cat > /tmp/a127-roles.sql <<EOF
CREATE ROLE oo_a127_reader LOGIN PASSWORD '${reader}';
CREATE ROLE oo_a127_writer LOGIN PASSWORD '${writer}';
EOF
  chmod 600 /tmp/a127-roles.sql
  psql -v ON_ERROR_STOP=1 -f /tmp/a127-roles.sql
  psql -v ON_ERROR_STOP=1 -f /fixture/init.sql
  rm -f /tmp/a127-roles.sql
fi
unset reader writer

psql -v ON_ERROR_STOP=1 -c "ALTER SYSTEM SET log_statement = 'all'"
psql -v ON_ERROR_STOP=1 -c "SELECT pg_catalog.pg_reload_conf()"

version=$(psql -At -c "SHOW server_version")
case "$version" in
  17.10*) ;;
  *) echo "server_version=$version" >&2; exit 1 ;;
esac
rows=$(psql -At -c "SELECT id::text || ':' || COALESCE(note, 'NULL') FROM oo_a127.cut ORDER BY id")
want=$'1:one\n2:NULL\n3:three-original'
if [ "$rows" != "$want" ]; then
  echo "seed mismatch" >&2
  exit 1
fi
reader_insert=$(psql -At -c "SELECT has_table_privilege('oo_a127_reader', 'oo_a127.cut', 'INSERT')")
reader_select=$(psql -At -c "SELECT has_table_privilege('oo_a127_reader', 'oo_a127.cut', 'SELECT')")
writer_insert=$(psql -At -c "SELECT has_table_privilege('oo_a127_writer', 'oo_a127.cut', 'INSERT')")
writer_update=$(psql -At -c "SELECT has_table_privilege('oo_a127_writer', 'oo_a127.cut', 'UPDATE')")
if [ "$reader_insert" != "f" ] || [ "$reader_select" != "t" ] || [ "$writer_insert" != "t" ] || [ "$writer_update" != "t" ]; then
  echo "privilege mismatch" >&2
  exit 1
fi
echo "init_ok version=$version"
