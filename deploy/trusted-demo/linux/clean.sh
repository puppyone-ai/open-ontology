#!/bin/bash
# Remove this UUID's containers, volumes, network, and private tmp.
# Does not prune and does not touch another owner.
set -eu
set -o pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a305
ROOT=/tmp/oo-a305-${RUN_ID}
NET=oo-a305-eef831aa-net
bash "$HERE/stop.sh"
vols=$(docker volume ls -q --filter label=oo.owner=a305 --filter label=oo.session="${RUN_ID}")
if [ -n "$vols" ]; then
  # shellcheck disable=SC2086
  docker volume rm $vols
fi
if docker network inspect "$NET" >/dev/null 2>&1; then
  owner=$(docker network inspect "$NET" --format '{{index .Labels "oo.owner"}}')
  session=$(docker network inspect "$NET" --format '{{index .Labels "oo.session"}}')
  if [ "$owner" = "a305" ] && [ "$session" = "$RUN_ID" ]; then
    docker network rm "$NET"
  else
    echo "refusing to remove network $NET" >&2
    exit 2
  fi
fi
case "$ROOT" in
  /tmp/oo-a305-eef831aa-dba4-4af2-a78b-541bc9e0a305)
    rm -rf "$ROOT"
    ;;
  *)
    echo "refusing to remove $ROOT" >&2
    exit 2
    ;;
esac
echo "cleaned_a305_private_resources"
