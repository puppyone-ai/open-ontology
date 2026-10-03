#!/bin/bash
# Peer auth inside the fixture. Role passwords are read from files, not argv.
set -eu
export PGHOST=/var/run/postgresql
export PGUSER=postgres
export PGDATABASE=oo_a141
reader=$(cat /secrets/reader.pass)
writer=$(cat /secrets/writer.pass)
have_reader=$(psql -At -c "SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_a141_reader'")
if [ "$have_reader" != "1" ]; then
  umask 077
  cat > /tmp/a141-roles.sql <<EOF
CREATE ROLE oo_a141_reader LOGIN PASSWORD '${reader}';
CREATE ROLE oo_a141_writer LOGIN PASSWORD '${writer}';
EOF
  chmod 600 /tmp/a141-roles.sql
  psql -v ON_ERROR_STOP=1 -f /tmp/a141-roles.sql
  rm -f /tmp/a141-roles.sql
fi
unset reader writer
psql -v ON_ERROR_STOP=1 -c "ALTER ROLE oo_a141_reader RESET log_statement"
psql -v ON_ERROR_STOP=1 -c "ALTER SYSTEM SET log_statement = 'all'"
psql -v ON_ERROR_STOP=1 -c "SELECT pg_catalog.pg_reload_conf()"
: > /pglog/postgresql.log
version=$(psql -At -c "SHOW server_version")
case "$version" in
  17.10*) ;;
  *) echo "server_version=$version" >&2; exit 1 ;;
esac
echo "init_ok version=$version"
