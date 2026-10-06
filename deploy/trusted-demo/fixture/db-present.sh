#!/bin/bash
# Prints 1 when the source database exists. Password bytes stay in this container.
set -eu
admin=$(tr -d '\r\n' < /secrets/admin.pass)
printf '%s:%s:%s:%s:%s\n' '*' '*' '*' oo_fixture_admin "$admin" > /tmp/a305-pgpass
chmod 600 /tmp/a305-pgpass
export PGPASSFILE=/tmp/a305-pgpass
psql -h 127.0.0.1 -U oo_fixture_admin -d postgres -Atqc "SELECT 1 FROM pg_database WHERE datname = 'oo_a305_src'"
