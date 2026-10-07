#!/bin/bash
# Remove this manifest's containers, volumes, network, and private tmp.
# Does not prune and does not touch another owner.
set -eu
set -o pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
DEMO=$(cd "$HERE/.." && pwd)
REPO=$(cd "$DEMO/../.." && pwd)
# shellcheck source=manifest.sh
. "$HERE/manifest.sh"
load_deployment "${OO_DEMO_MANIFEST:-}"
bind_resources "$REPO"
require_durable
bash "$HERE/stop.sh"
vols=$(docker volume ls -q --filter label=oo.project="$PROJECT" --filter label=oo.owner="$OWNER" --filter label=oo.session="$RUN_ID" --filter label=oo.fixed="$FIXED_SOURCE")
if [ -n "$vols" ]; then
  for vol in $vols; do
    state=$(object_state volume "$vol")
    if [ "$state" != "ours" ]; then
      echo "foreign labels on volume $vol; refusing" >&2
      exit 2
    fi
  done
  # shellcheck disable=SC2086
  docker volume rm $vols
fi
state=$(object_state network "$NET")
case "$state" in
  missing) ;;
  ours) docker network rm "$NET" >/dev/null ;;
  foreign)
    echo "foreign labels on network $NET; refusing" >&2
    exit 2
    ;;
esac
safe="/tmp/oo-${OWNER}-${RUN_ID}"
if [ "$ROOT" != "$safe" ]; then
  echo "refusing to remove $ROOT" >&2
  exit 2
fi
if [ -e "$ROOT" ]; then
  rm -rf "$ROOT"
fi
echo "cleaned_owned_private_resources"
