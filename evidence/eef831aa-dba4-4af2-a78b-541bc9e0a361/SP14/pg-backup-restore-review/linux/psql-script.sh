#!/bin/bash
# Apply one SQL file in a single transaction. The password stays in this container.
set -euo pipefail
db=$1
file=$2
trap 'rm -f /tmp/a361.pgpass' EXIT
umask 077
pass=$(tr -d '\r\n' < /var/lib/postgresql/a361-admin.pass)
printf '%s\n' "/var/run/postgresql:5432:*:oo_a356_admin:${pass}" > /tmp/a361.pgpass
chmod 600 /tmp/a361.pgpass
export PGPASSFILE=/tmp/a361.pgpass
export PGHOST=/var/run/postgresql
export PGUSER=oo_a356_admin
export PGPORT=5432
export PGCLIENTENCODING=UTF8
export PGOPTIONS='-c statement_timeout=180000 -c DateStyle=ISO,YMD -c TimeZone=UTC'
psql --no-password --no-psqlrc --quiet --single-transaction -v ON_ERROR_STOP=1 -d "$db" -f "$file"
