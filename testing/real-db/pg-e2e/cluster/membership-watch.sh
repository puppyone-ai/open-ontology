#!/bin/bash
# Host-side wait. Password expansion happens inside the container.
# Writes the ready file the inspect phase polls.
set -eu
NAME=oo-a317-pg
READY=$1
LOG=$(dirname "$READY")
mkdir -p "$LOG"
for _ in $(seq 1 14400); do
  if docker exec "$NAME" bash /tmp/a317-grant-runtime.sh > "$LOG/grant.out" 2>> "$LOG/grant.err"; then
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
