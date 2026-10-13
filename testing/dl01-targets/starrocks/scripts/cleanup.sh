#!/bin/sh
# Remove only this task's container and network. The pinned image stays.
# Refuse a container that does not carry this task's prefix label.
set -u
PREFIX=oo-dl01w1-13
CONTAINER="${PREFIX}-starrocks"
NETWORK="${PREFIX}"

if docker inspect "$CONTAINER" >/dev/null 2>&1; then
  label=$(docker inspect -f '{{index .Config.Labels "oo.prefix"}}' "$CONTAINER" 2>/dev/null || true)
  if [ "$label" = "$PREFIX" ]; then
    docker rm -f "$CONTAINER"
  else
    echo "refuse container ${CONTAINER}: prefix label is '${label}'" >&2
    exit 2
  fi
fi

if docker network inspect "$NETWORK" >/dev/null 2>&1; then
  nlabel=$(docker network inspect -f '{{index .Labels "oo.prefix"}}' "$NETWORK" 2>/dev/null || true)
  if [ "$nlabel" = "$PREFIX" ]; then
    docker network rm "$NETWORK"
  else
    echo "refuse network ${NETWORK}: prefix label is '${nlabel}'" >&2
    exit 2
  fi
fi

docker ps -a --filter "name=${PREFIX}" --format '{{.Names}} {{.Status}}'
echo "cleanup-done"
