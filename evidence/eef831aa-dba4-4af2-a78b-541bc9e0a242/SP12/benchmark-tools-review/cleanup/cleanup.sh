#!/bin/bash
# Remove only this review's containers, volumes, network, and tmp dir.
# Does not prune. Does not stop foreign containers. Does not delete images.
set -u
NAME=oo-a242-pg
NET=oo-a242-eef831aa-net
TMP=/tmp/oo-a242-eef831aa-dba4-4af2-a78b-541bc9e0a242
VOLS="oo-a242-eef831aa-cargo oo-a242-eef831aa-target oo-a242-eef831aa-pgdata"

if docker ps -a --filter "name=^/${NAME}$" --format '{{.Names}}' | grep -q "^${NAME}$"; then
  docker stop "$NAME" >/dev/null
  docker rm "$NAME" >/dev/null
  echo "removed container $NAME"
else
  echo "container $NAME absent"
fi

for vol in $VOLS; do
  if docker volume inspect "$vol" >/dev/null 2>&1; then
    docker volume rm "$vol" >/dev/null
    echo "removed volume $vol"
  else
    echo "volume $vol absent"
  fi
done

if docker network inspect "$NET" >/dev/null 2>&1; then
  docker network rm "$NET" >/dev/null
  echo "removed network $NET"
else
  echo "network $NET absent"
fi

if [ -d "$TMP" ]; then
  rm -rf "$TMP"
  echo "removed tmp"
else
  echo "tmp absent"
fi
echo cleanup_ok
