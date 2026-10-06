#!/bin/bash
# Remove only a268 resources for this session. No prune. No other owners.
set -u
OWNER=a268
SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a268
ROOT=/tmp/oo-a268-${SESSION}
REVIEW=/Users/redux/puppy-one/open-ontology/evidence/${SESSION}/SP11/pg-onboarding-outcome-repair
LOG=${REVIEW}/cleanup.log
exec > >(tee "$LOG") 2>&1
echo "cleanup_start $(date -u +%Y-%m-%dT%H:%M:%SZ)"

remove_container() {
  name=$1
  if ! docker container inspect "$name" >/dev/null 2>&1; then
    echo "container_absent $name"
    return 0
  fi
  owner=$(docker inspect -f '{{index .Config.Labels "oo.owner"}}' "$name")
  session=$(docker inspect -f '{{index .Config.Labels "oo.session"}}' "$name")
  if [ "$owner" != "$OWNER" ] || [ "$session" != "$SESSION" ]; then
    echo "refusing container $name owner=$owner session=$session" >&2
    exit 2
  fi
  docker stop "$name" >/dev/null 2>&1 || true
  docker rm "$name"
  echo "container_removed $name"
}

remove_container oo-a268-smoke
remove_container oo-a268-red
remove_container oo-a268-red-2
remove_container oo-a268-red-3
remove_container oo-a268-red-4
remove_container oo-a268-green
remove_container oo-a268-manifest

for vol in oo-a268-eef831aa-cargo oo-a268-eef831aa-target; do
  if ! docker volume inspect "$vol" >/dev/null 2>&1; then
    echo "volume_absent $vol"
    continue
  fi
  owner=$(docker volume inspect -f '{{index .Labels "oo.owner"}}' "$vol")
  session=$(docker volume inspect -f '{{index .Labels "oo.session"}}' "$vol")
  if [ "$owner" != "$OWNER" ] || [ "$session" != "$SESSION" ]; then
    echo "refusing volume $vol owner=$owner session=$session" >&2
    exit 2
  fi
  docker volume rm "$vol"
  echo "volume_removed $vol"
done

net=oo-a268-eef831aa-net
if docker network inspect "$net" >/dev/null 2>&1; then
  owner=$(docker network inspect -f '{{index .Labels "oo.owner"}}' "$net")
  session=$(docker network inspect -f '{{index .Labels "oo.session"}}' "$net")
  if [ "$owner" != "$OWNER" ] || [ "$session" != "$SESSION" ]; then
    echo "refusing network $net owner=$owner session=$session" >&2
    exit 2
  fi
  docker network rm "$net"
  echo "network_removed $net"
else
  echo "network_absent $net"
fi

if [ -d "$ROOT" ]; then
  rm -rf "$ROOT"
  echo "tmp_removed $ROOT"
else
  echo "tmp_absent $ROOT"
fi

echo "remaining_a268"
docker ps -a --filter label=oo.owner=a268 --format '{{.Names}} {{.Status}}' || true
echo "cleanup_done"
