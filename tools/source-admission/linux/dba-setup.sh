#!/bin/bash
# Runs inside the PostgreSQL container as the DBA role. Not the reader.
# create-role.sql is a private file generated outside Git. This script does
# not print it.
set -eu
export PGPASSFILE=/secrets/dba.pgpass
psql -w -h /var/run/postgresql -U oo_dl01w1_02_dba -d src -v ON_ERROR_STOP=1 \
  -f /secrets/create-role.sql >/secrets/create-role.out 2>&1
psql -w -h /var/run/postgresql -U oo_dl01w1_02_dba -d src -v ON_ERROR_STOP=1 \
  -f /dba/dba-grants.sql
