#!/bin/bash
# Load the private synthetic fixture through psql inside the owned PostgreSQL container.
# Password bytes stay in the container. This script's argv has paths only.
set -eu
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder
NAME=oo-a328-pg
PG=${EVIDENCE}/pg
FIXTURE=/tmp/oo-a328-eef831aa-dba4-4af2-a78b-541bc9e0a328/fixture
docker cp "$PG/inside-load.sh" "$NAME":/tmp/a328-inside-load.sh
docker cp "$FIXTURE/schema.sql" "$NAME":/tmp/a328-schema.sql
docker cp "$FIXTURE/party.copy" "$NAME":/tmp/a328-party.copy
docker cp "$FIXTURE/account.copy" "$NAME":/tmp/a328-account.copy
docker cp "$FIXTURE/entry.copy" "$NAME":/tmp/a328-entry.copy
docker exec "$NAME" bash /tmp/a328-inside-load.sh > "$PG/server.txt"
docker cp "$NAME":/tmp/a328-grants.txt "$PG/grants-before.txt"
docker cp "$NAME":/tmp/a328-counts.txt "$PG/row-counts.txt"
docker cp "$NAME":/tmp/a328-reltuples.txt "$PG/reltuples.txt"
grep -q '^autovacuum=off$' "$PG/server.txt"
grep -Eq '^ssl=(true|t)$' "$PG/server.txt"
grep -q '^server_version=17.10' "$PG/server.txt"
grep -q '^party=2000$' "$PG/row-counts.txt"
grep -q '^account=8000$' "$PG/row-counts.txt"
grep -q '^entry=90000$' "$PG/row-counts.txt"
echo "fixture_loaded"
