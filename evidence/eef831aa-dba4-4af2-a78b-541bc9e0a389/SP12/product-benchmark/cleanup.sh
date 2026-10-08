#!/bin/bash
# Remove only a389 resources for this session. No prune. No other owners.
set -u
OWNER=a389
SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a389
ROOT=/tmp/oo-a389-${SESSION}
REVIEW=/Users/redux/puppy-one/open-ontology/evidence/${SESSION}/SP12/product-benchmark
LOG=${REVIEW}/cleanup.log
exec > >(tee -a "$LOG") 2>&1
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

remove_container oo-a389-smoke
remove_container oo-a389-focused
remove_container oo-a389-chain
remove_container oo-a389-bench
remove_container oo-a389-hit
remove_container oo-a389-pg
remove_container oo-a389-propose

for id in $(docker ps -aq --filter label=oo.owner=${OWNER} --filter label=oo.session=${SESSION}); do
  name=$(docker inspect -f '{{.Name}}' "$id" | sed 's#^/##')
  remove_container "$name"
done

for vol in oo-a389-eef831aa-cargo oo-a389-eef831aa-target oo-a389-eef831aa-pgdata; do
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

net=oo-a389-eef831aa-net
if docker network inspect "$net" >/dev/null 2>&1; then
  owner=$(docker network inspect -f '{{index .Labels "oo.owner"}}' "$net")
  session=$(docker network inspect -f '{{index .Labels "oo.session"}}' "$net")
  if [ "$owner" != "$OWNER" ] || [ "$session" != "$SESSION" ]; then
    echo "refusing network $net owner=$owner session=$session" >&2
    exit 2
  fi
  foreign=$(docker network inspect "$net" --format '{{range $id, $c := .Containers}}{{$c.Name}}{{"\n"}}{{end}}')
  if [ -n "$foreign" ]; then
    echo "refusing network $net still attached: $foreign" >&2
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
echo "cleanup_done $(date -u +%Y-%m-%dT%H:%M:%SZ)"
