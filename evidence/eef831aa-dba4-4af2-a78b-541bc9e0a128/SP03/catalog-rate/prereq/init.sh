#!/bin/bash
# Inside the fixture as the postgres OS user. Peer auth. No argv secrets.
# Roles are created before log_statement=all.
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
cat > /tmp/a125-roles.sql <<EOF
CREATE ROLE oo_snap_reader LOGIN PASSWORD '${reader}';
CREATE ROLE oo_snap_writer LOGIN PASSWORD '${writer}';
CREATE ROLE oo_snap_guc LOGIN PASSWORD '${guc}';
CREATE ROLE lot_reader LOGIN NOSUPERUSER NOBYPASSRLS NOINHERIT NOCREATEDB NOCREATEROLE PASSWORD '${lot_reader}';
CREATE ROLE lot_writer LOGIN NOSUPERUSER NOBYPASSRLS NOINHERIT NOCREATEDB NOCREATEROLE PASSWORD '${lot_writer}';
EOF
chmod 600 /tmp/a125-roles.sql
psql -v ON_ERROR_STOP=1 -f /tmp/a125-roles.sql
psql -v ON_ERROR_STOP=1 -f /fixture/init.sql
rm -f /tmp/a125-roles.sql
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
psql -v ON_ERROR_STOP=1 -At -c "SHOW server_version"
