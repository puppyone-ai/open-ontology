#!/bin/bash
set -eu
umask 077
trap 'rm -f /tmp/a396.pgpass' EXIT
printf 'pg:5432:oo_a390_control:oo_a390_runtime_login:%s\n' "$(cat /secrets/runtime.pass)" > /tmp/a396.pgpass
chmod 600 /tmp/a396.pgpass
export PGPASSFILE=/tmp/a396.pgpass
export PGSSLMODE=verify-full
export PGSSLROOTCERT=/secrets/ca.crt
export PGHOST=pg
export PGPORT=5432
export PGDATABASE=oo_a390_control
export PGUSER=oo_a390_runtime_login
psql -v ON_ERROR_STOP=1 -X -f /oracle/after.sql
