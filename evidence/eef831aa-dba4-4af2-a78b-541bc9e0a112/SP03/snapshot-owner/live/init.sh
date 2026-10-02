#!/bin/bash
# Runs inside the fixture container as the postgres OS user. Peer auth. No argv secrets.
set -eu
export PGHOST=/var/run/postgresql
export PGUSER=postgres
export PGDATABASE=snapdb
reader=$(cat /secrets/reader.pass)
writer=$(cat /secrets/writer.pass)
guc=$(cat /secrets/guc.pass)
umask 077
cat > /tmp/snap-roles.sql <<EOF
CREATE ROLE oo_snap_reader LOGIN PASSWORD '${reader}';
CREATE ROLE oo_snap_writer LOGIN PASSWORD '${writer}';
CREATE ROLE oo_snap_guc LOGIN PASSWORD '${guc}';
EOF
chmod 600 /tmp/snap-roles.sql
psql -v ON_ERROR_STOP=1 -f /tmp/snap-roles.sql
psql -v ON_ERROR_STOP=1 -f /fixture/init.sql
rm -f /tmp/snap-roles.sql
psql -v ON_ERROR_STOP=1 -c "ALTER SYSTEM SET log_statement = 'all'"
psql -v ON_ERROR_STOP=1 -c "SELECT pg_catalog.pg_reload_conf()"
: > /pglog/postgresql.log
sleep 1
: > /pglog/postgresql.log
leak=0
for file in /secrets/reader.pass /secrets/writer.pass /secrets/guc.pass /secrets/super.pass; do
  secret=$(cat "$file")
  if [ -n "$secret" ] && grep -F -q -- "$secret" /pglog/postgresql.log; then
    leak=1
  fi
done
if [ "$leak" -ne 0 ]; then
  echo "log_leak=yes" >&2
  exit 1
fi
echo "log_leak=no"
