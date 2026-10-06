#!/bin/bash
# Runs inside the PostgreSQL container after init-pg has created oo_pipeline_runtime.
# Exits 1 until that role exists. Does not print password bytes.
# This grant is fixture configuration. It is not a business-column grant and not authentication.
set -eu
umask 077
admin=$(tr -d '\r\n' < /secrets/admin.pass)
printf '%s:%s:%s:%s:%s\n' '*' '*' '*' oo_fixture_admin "$admin" > /tmp/a305-pgpass
chmod 600 /tmp/a305-pgpass
export PGPASSFILE=/tmp/a305-pgpass
PSQL=(psql -h 127.0.0.1 -U oo_fixture_admin -v ON_ERROR_STOP=1 -d postgres)
found=$("${PSQL[@]}" -Atqc "SELECT 1 FROM pg_roles WHERE rolname = 'oo_pipeline_runtime'")
if [ "$found" != "1" ]; then
  exit 1
fi
"${PSQL[@]}" -c "GRANT CONNECT ON DATABASE oo_a305_dst TO oo_a305_runtime, oo_a305_login"
"${PSQL[@]}" -c "GRANT oo_pipeline_runtime TO oo_a305_runtime WITH INHERIT TRUE, SET FALSE"
echo granted
