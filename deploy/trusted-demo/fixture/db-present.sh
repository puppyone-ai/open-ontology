#!/bin/bash
# Prints 1 when the source database exists. Password bytes stay in this container.
set -eu
owner=${OO_DEMO_OWNER:-}
case "$owner" in
  [a-f0-9][a-f0-9][a-f0-9][a-f0-9]) ;;
  *)
    echo "owner token refused" >&2
    exit 2
    ;;
esac
admin=$(tr -d '\r\n' < /secrets/admin.pass)
printf '%s:%s:%s:%s:%s\n' '*' '*' '*' oo_fixture_admin "$admin" > /tmp/demo-pgpass
chmod 600 /tmp/demo-pgpass
export PGPASSFILE=/tmp/demo-pgpass
psql -h 127.0.0.1 -U oo_fixture_admin -d postgres -Atqc "SELECT 1 FROM pg_database WHERE datname = 'oo_${owner}_src'"
