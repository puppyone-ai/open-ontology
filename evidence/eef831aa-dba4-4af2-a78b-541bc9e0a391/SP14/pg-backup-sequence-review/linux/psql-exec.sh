#!/bin/bash
# Run one SQL file as the frozen admin. The password stays in this container.
# A second argument is a file name under /run-in, so parallel probes do not share one file.
set -euo pipefail
db=$1
sqlname=${2:-query.sql}
case "$sqlname" in
  ''|*[!A-Za-z0-9._-]*|.*)
    echo "sql name refused" >&2
    exit 2
    ;;
esac
umask 077
passfile=$(mktemp /tmp/a391.pgpass.XXXXXX)
trap 'rm -f "$passfile"' EXIT
pass=$(tr -d '\r\n' < /var/lib/postgresql/a391-admin.pass)
printf '%s\n' "/var/run/postgresql:5432:*:oo_a356_admin:${pass}" > "$passfile"
chmod 600 "$passfile"
export PGPASSFILE=$passfile
export PGHOST=/var/run/postgresql
export PGUSER=oo_a356_admin
export PGPORT=5432
export PGCLIENTENCODING=UTF8
export PGOPTIONS='-c statement_timeout=180000 -c DateStyle=ISO,YMD -c TimeZone=UTC'
psql --no-password --no-psqlrc --tuples-only --no-align --quiet -v ON_ERROR_STOP=1 -d "$db" -f "/run-in/${sqlname}"
