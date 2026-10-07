#!/bin/bash
# Load schema and COPY text through psql inside the PostgreSQL container.
# Password bytes stay in the container. This script's argv has paths only.
set -eu
SRC=/Users/redux/puppy-one/open-ontology
FIXTURE=${SRC}/testing/real-db/pg-e2e
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a317/SP11/real-pg-e2e/in-approved-rerun
NAME=oo-a317-pg
PG=${EVIDENCE}/pg
READY=/tmp/oo-a317-eef831aa-dba4-4af2-a78b-541bc9e0a317/ready
docker cp "$FIXTURE/cluster/inside-cluster.sh" "$NAME":/tmp/a317-inside-cluster.sh
docker cp "$FIXTURE/cluster/grant-runtime.sh" "$NAME":/tmp/a317-grant-runtime.sh
docker cp "$FIXTURE/fixture/schema.sql" "$NAME":/tmp/a317-schema.sql
docker cp "$EVIDENCE/live/fixture/party.copy" "$NAME":/tmp/a317-party.copy
docker cp "$EVIDENCE/live/fixture/account.copy" "$NAME":/tmp/a317-account.copy
docker cp "$EVIDENCE/live/fixture/entry.copy" "$NAME":/tmp/a317-entry.copy
docker exec "$NAME" bash /tmp/a317-inside-cluster.sh > "$PG/server.txt"
grep -q '^autovacuum=off$' "$PG/server.txt"
grep -Eq '^ssl=(true|t)$' "$PG/server.txt"
grep -q '^server_version=17.10' "$PG/server.txt"
mkdir -p "$READY"
echo "fixture_loaded"
