#!/bin/bash
# Remove containers whose four deployment labels match. Keep volumes, secrets, and evidence.
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
safe="/tmp/oo-${OWNER}-${RUN_ID}"
if [ "$ROOT" != "$safe" ]; then
  echo "refusing to read watch pid outside the private directory" >&2
  exit 2
fi
if [ -f "$ROOT/watch.pid" ]; then
  pid=$(tr -d '[:space:]' < "$ROOT/watch.pid")
  case "$pid" in
    ''|*[!0-9]*) ;;
    *)
      cmd=$(ps -p "$pid" -o command= 2>/dev/null || true)
      case "$cmd" in
        *membership-watch.sh*)
          kill "$pid" 2>/dev/null || true
          wait "$pid" 2>/dev/null || true
          ;;
      esac
      ;;
  esac
fi
ids=$(docker ps -aq --filter label=oo.project="$PROJECT" --filter label=oo.owner="$OWNER" --filter label=oo.session="$RUN_ID" --filter label=oo.fixed="$FIXED_SOURCE")
if [ -n "$ids" ]; then
  for id in $ids; do
    state=$(object_state container "$id")
    if [ "$state" != "ours" ]; then
      echo "foreign labels on container $id; refusing" >&2
      exit 2
    fi
  done
  # shellcheck disable=SC2086
  docker rm -f $ids
fi
echo "stopped_owned_containers"
