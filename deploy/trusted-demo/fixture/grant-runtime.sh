#!/bin/bash
# Runs inside the PostgreSQL container after init-pg has created oo_pipeline_runtime.
# Exits 1 until that role exists. Does not print password bytes.
# This grant is fixture configuration. It is not a business-column grant and not authentication.
set -eu
owner=${OO_DEMO_OWNER:-}
case "$owner" in
  [a-f0-9][a-f0-9][a-f0-9][a-f0-9]) ;;
  *)
    echo "owner token refused" >&2
    exit 2
    ;;
esac
umask 077
admin=$(tr -d '\r\n' < /secrets/admin.pass)
printf '%s:%s:%s:%s:%s\n' '*' '*' '*' oo_fixture_admin "$admin" > /tmp/demo-pgpass
chmod 600 /tmp/demo-pgpass
export PGPASSFILE=/tmp/demo-pgpass
runtime_role="oo_${owner}_runtime"
login_role="oo_${owner}_login"
db_dst="oo_${owner}_dst"
PSQL=(psql -h 127.0.0.1 -U oo_fixture_admin -v ON_ERROR_STOP=1 -d postgres)
found=$("${PSQL[@]}" -Atqc "SELECT 1 FROM pg_roles WHERE rolname = 'oo_pipeline_runtime'")
if [ "$found" != "1" ]; then
  exit 1
fi
"${PSQL[@]}" -c "GRANT CONNECT ON DATABASE ${db_dst} TO ${runtime_role}, ${login_role}"
"${PSQL[@]}" -c "GRANT oo_pipeline_runtime TO ${runtime_role} WITH INHERIT TRUE, SET FALSE"
echo granted
