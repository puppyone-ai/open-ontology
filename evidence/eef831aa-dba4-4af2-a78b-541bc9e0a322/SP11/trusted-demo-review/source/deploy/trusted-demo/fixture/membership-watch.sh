#!/bin/bash
# Host-side wait. Password expansion happens inside the container.
# Writes the ready file the inspect phase polls. Does not GRANT from the CLI.
set -eu
READY=${1:-}
NAME=${2:-}
case "$NAME" in
  oo-[a-f0-9][a-f0-9][a-f0-9][a-f0-9]-pg) ;;
  *)
    echo "postgres name refused" >&2
    exit 2
    ;;
esac
if [ -z "$READY" ] || [ -z "${OWNER:-}" ] || [ -z "${RUN_ID:-}" ] || [ -z "${PROJECT:-}" ] || [ -z "${FIXED_SOURCE:-}" ]; then
  echo "membership watch is missing deployment labels" >&2
  exit 2
fi
project=$(docker inspect "$NAME" --format '{{index .Config.Labels "oo.project"}}')
owner=$(docker inspect "$NAME" --format '{{index .Config.Labels "oo.owner"}}')
session=$(docker inspect "$NAME" --format '{{index .Config.Labels "oo.session"}}')
fixed=$(docker inspect "$NAME" --format '{{index .Config.Labels "oo.fixed"}}')
if [ "$project" != "$PROJECT" ] || [ "$owner" != "$OWNER" ] || [ "$session" != "$RUN_ID" ] || [ "$fixed" != "$FIXED_SOURCE" ]; then
  echo "foreign labels on $NAME; refusing membership watch" >&2
  exit 2
fi
case "$owner" in
  [a-f0-9][a-f0-9][a-f0-9][a-f0-9]) ;;
  *)
    echo "owner token refused" >&2
    exit 2
    ;;
esac
LOG=$(dirname "$READY")
mkdir -p "$LOG"
for _ in $(seq 1 14400); do
  if docker exec -e OO_DEMO_OWNER="$owner" "$NAME" bash /tmp/grant-runtime.sh > "$LOG/grant.out" 2>> "$LOG/grant.err"; then
    if grep -q '^granted$' "$LOG/grant.out"; then
      : > "$READY"
      echo "membership_ready"
      exit 0
    fi
  fi
  sleep 1
done
echo "membership watcher timed out" >&2
exit 1
