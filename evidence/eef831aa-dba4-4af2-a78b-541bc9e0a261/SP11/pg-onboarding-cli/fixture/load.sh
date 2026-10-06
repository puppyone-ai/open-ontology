#!/bin/bash
# Load the fresh fixture through psql inside the PostgreSQL container.
# Password bytes stay in the container. This script's argv has paths only.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a261
REPO=/Users/redux/puppy-one/open-ontology
ROOT=${REPO}/evidence/${RUN_ID}/SP11/pg-onboarding-cli
NAME=oo-a261-pg
PG=${ROOT}/pg
docker cp "$ROOT/fixture/inside-cluster.sh" "$NAME":/tmp/inside-cluster.sh
docker cp "$ROOT/fixture/grant-runtime.sh" "$NAME":/tmp/grant-runtime.sh
docker cp "$ROOT/fixture/source.sql" "$NAME":/tmp/source.sql
docker exec "$NAME" bash /tmp/inside-cluster.sh > "$PG/server.txt"
grep -q '^autovacuum=off$' "$PG/server.txt"
grep -Eq '^ssl=(true|t)$' "$PG/server.txt"
grep -q '^server_version=17.10' "$PG/server.txt"
echo "fixture_loaded"
