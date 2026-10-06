#!/bin/bash
# Remove containers labeled for this UUID. Keep volumes, secrets, and evidence.
set -eu
set -o pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a305
ROOT=/tmp/oo-a305-${RUN_ID}
if [ -f "$ROOT/watch.pid" ]; then
  pid=$(tr -d '[:space:]' < "$ROOT/watch.pid")
  if [ -n "$pid" ]; then
    cmd=$(ps -p "$pid" -o command= 2>/dev/null || true)
    case "$cmd" in
      *membership-watch.sh*)
        kill "$pid" 2>/dev/null || true
        wait "$pid" 2>/dev/null || true
        ;;
    esac
  fi
fi
ids=$(docker ps -aq --filter label=oo.owner=a305 --filter label=oo.session="${RUN_ID}")
if [ -n "$ids" ]; then
  # shellcheck disable=SC2086
  docker rm -f $ids
fi
echo "stopped_a305_containers"
