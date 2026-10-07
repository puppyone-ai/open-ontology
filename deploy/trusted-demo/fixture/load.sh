#!/bin/bash
# Load the fresh fixture through psql inside the PostgreSQL container.
# Password bytes stay in the container. This script's argv has paths only.
# A database that already exists is left in place.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
DEMO=$(cd "$HERE/.." && pwd)
REPO=$(cd "$DEMO/../.." && pwd)
# shellcheck source=../linux/manifest.sh
. "$DEMO/linux/manifest.sh"
load_deployment "${OO_DEMO_MANIFEST:-}"
bind_resources "$REPO"
EVIDENCE_PG=${EVIDENCE}/pg
PRIVATE=$ROOT
NAME=$PG_NAME
mkdir -p "$EVIDENCE_PG"
if [ ! -d "$PRIVATE" ]; then
  echo "private directory is absent" >&2
  exit 2
fi
docker cp "$HERE/inside-cluster.sh" "$NAME":/tmp/inside-cluster.sh
docker cp "$HERE/grant-runtime.sh" "$NAME":/tmp/grant-runtime.sh
docker cp "$HERE/db-present.sh" "$NAME":/tmp/db-present.sh
docker cp "$HERE/source.sql" "$NAME":/tmp/source.sql
present=$(docker exec -e OO_DEMO_OWNER="$OWNER" "$NAME" bash /tmp/db-present.sh 2>"$PRIVATE/db-present.err" | tr -d '[:space:]')
if [ "$present" = "1" ]; then
  printf '%s\n' present > "$EVIDENCE_PG/fixture-state.txt"
  echo "fixture_preserved"
  exit 0
fi
docker exec -e OO_DEMO_OWNER="$OWNER" "$NAME" bash /tmp/inside-cluster.sh > "$EVIDENCE_PG/server.txt" 2>"$PRIVATE/load.err"
grep -q '^autovacuum=off$' "$EVIDENCE_PG/server.txt"
grep -Eq '^ssl=(true|t)$' "$EVIDENCE_PG/server.txt"
grep -q '^server_version=17.10' "$EVIDENCE_PG/server.txt"
printf '%s\n' loaded > "$EVIDENCE_PG/fixture-state.txt"
echo "fixture_loaded"
