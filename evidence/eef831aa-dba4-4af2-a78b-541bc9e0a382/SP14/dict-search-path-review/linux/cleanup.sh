#!/bin/bash
# Remove only this session's containers, volumes, network, and tmp directory.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a361/SP14/pg-backup-restore-review/linux/cleanup.sh
# commit c870a52817d172d13b7f890548bf620d55a623e7
# sha256 1af543860e0413bbf6746aad10c766681e6e8c87f3483ef10eab9d4aae901788
# Change: a382 names. Does not prune. Does not remove other owners.
set -u
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a382
EVIDENCE=/Users/redux/puppy-one/open-ontology/evidence/${RUN_ID}/SP14/dict-search-path-review
mkdir -p "$EVIDENCE"
{
  echo "cleanup-start"
  docker ps -aq --filter label=oo.owner=a382 --filter label=oo.session="${RUN_ID}"
} > "$EVIDENCE/cleanup-targets.txt"
{
  echo "kept-before-removal"
  docker ps -a --format '{{.Names}} {{.Label "oo.owner"}} {{.Label "oo.session"}}'
} > "$EVIDENCE/cleanup-kept.txt"
for id in $(docker ps -aq --filter label=oo.owner=a382 --filter label=oo.session="${RUN_ID}"); do
  owner=$(docker inspect --format '{{index .Config.Labels "oo.owner"}}' "$id")
  session=$(docker inspect --format '{{index .Config.Labels "oo.session"}}' "$id")
  if [ "$owner" = "a382" ] && [ "$session" = "$RUN_ID" ]; then
    docker rm -f "$id"
  fi
done
for vol in oo-a382-eef831aa-cargo oo-a382-eef831aa-target oo-a382-eef831aa-pgdata; do
  if docker volume inspect "$vol" >/dev/null 2>&1; then
    owner=$(docker volume inspect --format '{{index .Labels "oo.owner"}}' "$vol")
    session=$(docker volume inspect --format '{{index .Labels "oo.session"}}' "$vol")
    if [ "$owner" = "a382" ] && [ "$session" = "$RUN_ID" ]; then
      docker volume rm "$vol"
    fi
  fi
done
if docker network inspect oo-a382-eef831aa-net >/dev/null 2>&1; then
  owner=$(docker network inspect --format '{{index .Labels "oo.owner"}}' oo-a382-eef831aa-net)
  session=$(docker network inspect --format '{{index .Labels "oo.session"}}' oo-a382-eef831aa-net)
  if [ "$owner" = "a382" ] && [ "$session" = "$RUN_ID" ]; then
    docker network rm oo-a382-eef831aa-net
  fi
fi
rm -rf "/tmp/oo-a382-${RUN_ID}"
{
  echo "remaining-a382"
  docker ps -aq --filter label=oo.owner=a382 --filter label=oo.session="${RUN_ID}"
  docker volume ls -q --filter label=oo.owner=a382 --filter label=oo.session="${RUN_ID}"
} >> "$EVIDENCE/cleanup-targets.txt"
printf '%s\n' done > "$EVIDENCE/cleanup-done.txt"
