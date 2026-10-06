#!/bin/bash
# Remove only this task's containers, volumes, network, and private tmp directory.
# Does not prune, does not delete pinned images, and does not touch another owner.
set -u
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a249
ROOT=/tmp/oo-a249-${RUN_ID}
REVIEW=/Users/redux/puppy-one/open-ontology/evidence/${RUN_ID}/SP06/mapping-reader-cli
LOG=${REVIEW}/cleanup/CLEANUP.txt
mkdir -p "${REVIEW}/cleanup"
{
  echo "owner=a249"
  echo "session=${RUN_ID}"
  echo "started=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
} > "$LOG"

for name in oo-a249-smoke oo-a249-chain oo-a249-review oo-a249-pg; do
  if docker container inspect "$name" >/dev/null 2>&1; then
    owner=$(docker inspect -f '{{index .Config.Labels "oo.owner"}}' "$name" 2>/dev/null || true)
    if [ "$owner" != "a249" ]; then
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

for vol in oo-a249-eef831aa-cargo oo-a249-eef831aa-target oo-a249-eef831aa-pgdata; do
  if docker volume inspect "$vol" >/dev/null 2>&1; then
    owner=$(docker volume inspect -f '{{index .Labels "oo.owner"}}' "$vol" 2>/dev/null || true)
    if [ "$owner" != "a249" ]; then
      echo "refusing volume $vol owner=$owner" | tee -a "$LOG"
      exit 2
    fi
    docker volume rm "$vol" >> "$LOG" 2>&1 || true
    echo "removed volume $vol" >> "$LOG"
  else
    echo "absent volume $vol" >> "$LOG"
  fi
done

if docker network inspect oo-a249-eef831aa-net >/dev/null 2>&1; then
  owner=$(docker network inspect -f '{{index .Labels "oo.owner"}}' oo-a249-eef831aa-net 2>/dev/null || true)
  if [ "$owner" != "a249" ]; then
    echo "refusing network owner=$owner" | tee -a "$LOG"
    exit 2
  fi
  docker network rm oo-a249-eef831aa-net >> "$LOG" 2>&1 || true
  echo "removed network oo-a249-eef831aa-net" >> "$LOG"
else
  echo "absent network oo-a249-eef831aa-net" >> "$LOG"
fi

if [ -d "$ROOT" ]; then
  rm -rf "$ROOT"
  echo "removed $ROOT" >> "$LOG"
else
  echo "absent $ROOT" >> "$LOG"
fi
echo "finished=$(date -u +%Y-%m-%dT%H:%M:%SZ)" >> "$LOG"
echo "cleanup_recorded"
