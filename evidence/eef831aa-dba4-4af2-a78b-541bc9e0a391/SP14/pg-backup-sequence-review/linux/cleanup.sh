#!/bin/bash
# Remove only this session's Docker objects. Does not prune.
set -uo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a391
EVIDENCE=/Users/redux/puppy-one/open-ontology/evidence/${RUN_ID}/SP14/pg-backup-sequence-review
mkdir -p "$EVIDENCE"
{
  echo "name status"
  docker ps -a --filter label=oo.owner=a391 --filter label=oo.session="${RUN_ID}" --format '{{.Names}} {{.Status}}'
} > "$EVIDENCE/owned-before-cleanup.txt"
docker ps -aq --filter label=oo.owner=a391 --filter label=oo.session="${RUN_ID}" | while read -r id; do
  [ -n "$id" ] || continue
  docker rm -f "$id" >/dev/null || true
done
for vol in oo-a391-eef831aa-pgdata oo-a391-eef831aa-cargo oo-a391-eef831aa-target; do
  if docker volume inspect "$vol" >/dev/null 2>&1; then
    owner=$(docker volume inspect --format '{{index .Labels "oo.owner"}}' "$vol")
    session=$(docker volume inspect --format '{{index .Labels "oo.session"}}' "$vol")
    if [ "$owner" = a391 ] && [ "$session" = "$RUN_ID" ]; then
      docker volume rm "$vol" >/dev/null || true
    fi
  fi
done
if docker network inspect oo-a391-eef831aa-net >/dev/null 2>&1; then
  owner=$(docker network inspect --format '{{index .Labels "oo.owner"}}' oo-a391-eef831aa-net)
  session=$(docker network inspect --format '{{index .Labels "oo.session"}}' oo-a391-eef831aa-net)
  if [ "$owner" = a391 ] && [ "$session" = "$RUN_ID" ]; then
    docker network rm oo-a391-eef831aa-net >/dev/null || true
  fi
fi
{
  echo "remaining"
  docker ps -a --filter label=oo.owner=a391 --filter label=oo.session="${RUN_ID}" --format '{{.Names}}'
  docker volume ls --filter label=oo.owner=a391 --filter label=oo.session="${RUN_ID}" --format '{{.Name}}'
  docker network ls --filter label=oo.owner=a391 --filter label=oo.session="${RUN_ID}" --format '{{.Name}}'
} > "$EVIDENCE/cleanup-done.txt"
if grep -v '^remaining$' "$EVIDENCE/cleanup-done.txt" | grep -q .; then
  echo "owned objects remain" >&2
  exit 1
fi
echo "cleanup finished"
