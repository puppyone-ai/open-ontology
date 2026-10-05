#!/bin/bash
# Remove only this task's containers, volumes, network, and private tmp directory.
# Does not prune, does not delete pinned images, and does not touch another owner.
set -u
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a226
ROOT=/tmp/oo-a226-${RUN_ID}
REVIEW=/Users/redux/puppy-one/open-ontology/evidence/${RUN_ID}/SP11/reviewed-pg-composition
LOG=${REVIEW}/cleanup/CLEANUP.txt
mkdir -p "${REVIEW}/cleanup"
{
  echo "owner=a226"
  echo "session=${RUN_ID}"
  echo "started=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
} > "$LOG"

for name in oo-a226-smoke oo-a226-chain oo-a226-pg; do
  if docker container inspect "$name" >/dev/null 2>&1; then
    owner=$(docker inspect -f '{{index .Config.Labels "oo.owner"}}' "$name" 2>/dev/null || true)
    if [ "$owner" != "a226" ]; then
      echo "refusing container $name owner=$owner" | tee -a "$LOG"
      exit 2
    fi
    docker stop "$name" >> "$LOG" 2>&1 || true
    docker rm "$name" >> "$LOG" 2>&1 || true
    echo "removed container $name" >> "$LOG"
  else
    echo "absent container $name" >> "$LOG"
  fi
done

for vol in oo-a226-eef831aa-cargo oo-a226-eef831aa-target oo-a226-eef831aa-pgdata; do
  if docker volume inspect "$vol" >/dev/null 2>&1; then
    owner=$(docker volume inspect -f '{{index .Labels "oo.owner"}}' "$vol" 2>/dev/null || true)
    if [ "$owner" != "a226" ]; then
      echo "refusing volume $vol owner=$owner" | tee -a "$LOG"
      exit 2
    fi
    docker volume rm "$vol" >> "$LOG" 2>&1 || true
    echo "removed volume $vol" >> "$LOG"
  else
    echo "absent volume $vol" >> "$LOG"
  fi
done

if docker network inspect oo-a226-eef831aa-net >/dev/null 2>&1; then
  owner=$(docker network inspect -f '{{index .Labels "oo.owner"}}' oo-a226-eef831aa-net 2>/dev/null || true)
  if [ "$owner" != "a226" ]; then
    echo "refusing network owner=$owner" | tee -a "$LOG"
    exit 2
  fi
  docker network rm oo-a226-eef831aa-net >> "$LOG" 2>&1 || true
  echo "removed network oo-a226-eef831aa-net" >> "$LOG"
else
  echo "absent network oo-a226-eef831aa-net" >> "$LOG"
fi

if [ -d "$ROOT" ]; then
  rm -rf "$ROOT"
  echo "removed $ROOT" >> "$LOG"
else
  echo "absent $ROOT" >> "$LOG"
fi
echo "finished=$(date -u +%Y-%m-%dT%H:%M:%SZ)" >> "$LOG"
echo "cleanup_recorded"
