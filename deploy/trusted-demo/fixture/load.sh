#!/bin/bash
# Load the fresh fixture through psql inside the PostgreSQL container.
# Password bytes stay in the container. This script's argv has paths only.
# A database that already exists is left in place.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a305
REPO=/Users/redux/puppy-one/open-ontology
ROOT=${REPO}/deploy/trusted-demo
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP11/trusted-demo/pg
PRIVATE=/tmp/oo-a305-${RUN_ID}
NAME=oo-a305-pg
mkdir -p "$EVIDENCE" "$PRIVATE"
docker cp "$ROOT/fixture/inside-cluster.sh" "$NAME":/tmp/inside-cluster.sh
docker cp "$ROOT/fixture/grant-runtime.sh" "$NAME":/tmp/grant-runtime.sh
docker cp "$ROOT/fixture/db-present.sh" "$NAME":/tmp/db-present.sh
docker cp "$ROOT/fixture/source.sql" "$NAME":/tmp/source.sql
present=$(docker exec "$NAME" bash /tmp/db-present.sh 2>"$PRIVATE/db-present.err" | tr -d '[:space:]')
if [ "$present" = "1" ]; then
  printf '%s\n' present > "$EVIDENCE/fixture-state.txt"
  echo "fixture_preserved"
  exit 0
fi
docker exec "$NAME" bash /tmp/inside-cluster.sh > "$EVIDENCE/server.txt" 2>"$PRIVATE/load.err"
grep -q '^autovacuum=off$' "$EVIDENCE/server.txt"
grep -Eq '^ssl=(true|t)$' "$EVIDENCE/server.txt"
grep -q '^server_version=17.10' "$EVIDENCE/server.txt"
printf '%s\n' loaded > "$EVIDENCE/fixture-state.txt"
echo "fixture_loaded"
