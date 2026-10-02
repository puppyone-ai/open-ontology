#!/bin/bash
# Remove only the PostgreSQL resources recorded by up.sh.
set -eu
STATE="${1:-/tmp/oo-a066-pg.state}"
if [ ! -f "$STATE" ]; then
  exit 0
fi
# shellcheck disable=SC1090
. "$STATE"
docker rm -f "$container" >/dev/null 2>&1 || true
docker network rm "$network" >/dev/null 2>&1 || true
docker volume rm "$volume" >/dev/null 2>&1 || true
if [ -n "${secrets:-}" ] && [ -d "$secrets" ]; then
  rm -rf "$secrets"
fi
rm -f "$STATE"
