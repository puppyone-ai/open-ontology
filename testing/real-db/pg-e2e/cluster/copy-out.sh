#!/bin/bash
# Administrator COPY of one physical shadow table. Password stays in pgpass.
set -eu
umask 077
table=$1
admin=$(tr -d '\r\n' < /secrets/admin.pass)
printf '%s:%s:%s:%s:%s\n' '*' '*' '*' oo_fixture_admin "$admin" > /tmp/a317-pgpass
chmod 600 /tmp/a317-pgpass
export PGPASSFILE=/tmp/a317-pgpass
exec psql -h 127.0.0.1 -U oo_fixture_admin -d oo_a317_dst -v ON_ERROR_STOP=1 -f "/tmp/a317-copy-${table}.sql"
