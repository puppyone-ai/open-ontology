#!/bin/bash
# Host-side start/stop for the one owned Postgres. The builder has no docker socket.
set -u
PHASE="$1"
NAME=oo-a304-pg
mkdir -p "$PHASE"
while true; do
  if [ -f "$PHASE/stop.request" ] && [ ! -f "$PHASE/stop.done" ]; then
    docker stop -t 2 "$NAME" >/dev/null 2>&1 || true
    date +%s > "$PHASE/stop.done"
  fi
  if [ -f "$PHASE/start.request" ] && [ ! -f "$PHASE/start.done" ]; then
    docker start "$NAME" >/dev/null 2>&1 || true
    ready=0
    for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 32 33 34 35 36 37 38 39 40 41 42 43 44 45 46 47 48 49 50; do
      if docker exec -u postgres "$NAME" pg_isready -U oo_fixture_admin -d oo_a304 >/dev/null 2>&1; then
        ready=1
        break
      fi
      sleep 0.2
    done
    if [ "$ready" -eq 1 ]; then
      date +%s > "$PHASE/start.done"
    fi
  fi
  if [ -f "$PHASE/builder.exit" ]; then
    if [ -f "$PHASE/stop.done" ] && [ ! -f "$PHASE/start.done" ]; then
      docker start "$NAME" >/dev/null 2>&1 || true
    fi
    exit 0
  fi
  sleep 0.05
done
