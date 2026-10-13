#!/bin/bash
# Remove only resources labeled for this task. The pinned image is kept.
set -euo pipefail
source "$(dirname -- "$0")/lib.sh"

require_label() {
  local kind=$1
  local name=$2
  local owner
  case "$kind" in
    container)
      owner=$(docker inspect -f '{{index .Config.Labels "oo.owner"}}' "$name")
      ;;
    volume)
      owner=$(docker volume inspect -f '{{index .Labels "oo.owner"}}' "$name")
      ;;
    network)
      owner=$(docker network inspect -f '{{index .Labels "oo.owner"}}' "$name")
      ;;
    *)
      echo "refusing unknown kind $kind" >&2
      exit 2
      ;;
  esac
  if [ "$owner" != "DL01-W1-08" ]; then
    echo "refusing $kind $name owner=$owner" >&2
    exit 2
  fi
}

if docker ps -a --format '{{.Names}}' | grep -qx "$OO_CONTAINER"; then
  require_label container "$OO_CONTAINER"
  docker rm -f "$OO_CONTAINER" >/dev/null
  echo "removed_container=$OO_CONTAINER"
fi
if docker volume inspect "$OO_VOLUME" >/dev/null 2>&1; then
  require_label volume "$OO_VOLUME"
  docker volume rm "$OO_VOLUME" >/dev/null
  echo "removed_volume=$OO_VOLUME"
fi
if docker network inspect "$OO_NETWORK" >/dev/null 2>&1; then
  require_label network "$OO_NETWORK"
  docker network rm "$OO_NETWORK" >/dev/null
  echo "removed_network=$OO_NETWORK"
fi

if [ "${OO_KEEP_SECRETS:-}" != "1" ] && [ -d "$OO_SECRET_DIR" ]; then
  find "$OO_SECRET_DIR" -type f -exec rm -f {} +
  rm -rf "$OO_SECRET_DIR"
fi

echo "remaining_containers=$(docker ps -a --filter name=oo-dl01w1-08 --format '{{.Names}}' | tr '\n' ' ')"
echo "remaining_volumes=$(docker volume ls --filter name=oo-dl01w1-08 --format '{{.Name}}' | tr '\n' ' ')"
echo "remaining_networks=$(docker network ls --filter name=oo-dl01w1-08 --format '{{.Name}}' | tr '\n' ' ')"
if docker ps -a --filter name=oo-dl01w1-08 --format '{{.Names}}' | grep -q .; then
  exit 1
fi
if docker volume ls --filter name=oo-dl01w1-08 --format '{{.Name}}' | grep -q .; then
  exit 1
fi
if docker network ls --format '{{.Name}}' | grep -qx "$OO_NETWORK"; then
  exit 1
fi
if [ -d "$OO_SECRET_DIR" ]; then
  echo "secrets_dir_still_present" >&2
  exit 1
fi
docker image inspect "$OO_IMAGE_DIGEST" --format 'retained_image={{.Id}} arch={{.Architecture}}' 
echo "cleanup=ok"
