#!/bin/bash
# Admin schema change. The reader role is not used here.
set -eu
export PGHOST=/var/run/postgresql
export PGUSER=postgres
export PGDATABASE=oo_a157
psql -v ON_ERROR_STOP=1 -c "ALTER TABLE ledger.party ADD COLUMN note text"
echo "alter_ok"
