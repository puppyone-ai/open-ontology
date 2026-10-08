#!/bin/bash
# Remove only this session's containers, volumes, and network. Does not prune.
set -u
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a372
EVIDENCE=/Users/redux/puppy-one/open-ontology/evidence/${RUN_ID}/SP14/pg-backup-cut-review
mkdir -p "$EVIDENCE"
{
  echo "cleanup-start"
  docker ps -aq --filter label=oo.owner=a372 --filter label=oo.session="${RUN_ID}"
} > "$EVIDENCE/cleanup-targets.txt"
for id in $(docker ps -aq --filter label=oo.owner=a372 --filter label=oo.session="${RUN_ID}"); do
  owner=$(docker inspect --format '{{index .Config.Labels "oo.owner"}}' "$id")
  session=$(docker inspect --format '{{index .Config.Labels "oo.session"}}' "$id")
  if [ "$owner" = "a372" ] && [ "$session" = "$RUN_ID" ]; then
    docker rm -f "$id"
  fi
done
for vol in oo-a372-eef831aa-cargo oo-a372-eef831aa-target oo-a372-eef831aa-pgdata; do
  if docker volume inspect "$vol" >/dev/null 2>&1; then
    owner=$(docker volume inspect --format '{{index .Labels "oo.owner"}}' "$vol")
    session=$(docker volume inspect --format '{{index .Labels "oo.session"}}' "$vol")
    if [ "$owner" = "a372" ] && [ "$session" = "$RUN_ID" ]; then
      docker volume rm "$vol"
    fi
  fi
done
if docker network inspect oo-a372-eef831aa-net >/dev/null 2>&1; then
  owner=$(docker network inspect --format '{{index .Labels "oo.owner"}}' oo-a372-eef831aa-net)
  session=$(docker network inspect --format '{{index .Labels "oo.session"}}' oo-a372-eef831aa-net)
  if [ "$owner" = "a372" ] && [ "$session" = "$RUN_ID" ]; then
    docker network rm oo-a372-eef831aa-net
  fi
fi
rm -rf "/tmp/oo-a372-${RUN_ID}"
printf '%s\n' done > "$EVIDENCE/cleanup-done.txt"
