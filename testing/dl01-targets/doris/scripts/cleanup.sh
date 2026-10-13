#!/bin/bash
# Stop only this task's containers and network. Images are retained.
set -euo pipefail
# shellcheck disable=SC1091
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

DIR="$RAW/cleanup"
mkdir -p "$DIR"
{
  echo "before"
  date -u +%Y-%m-%dT%H:%M:%SZ
  docker ps -a --filter "name=oo-dl01w1-12" --format '{{.ID}} {{.Names}} {{.Status}} {{.Label "oo.owner"}}'
} >"$DIR/before.txt"

docker stop oo-dl01w1-12-cancel-client >"$DIR/stop-client.stdout" 2>"$DIR/stop-client.stderr" || true
docker rm -f oo-dl01w1-12-cancel-client >"$DIR/rm-client.stdout" 2>"$DIR/rm-client.stderr" || true
docker stop "$BE_NAME" >"$DIR/stop-be.stdout" 2>"$DIR/stop-be.stderr" || true
docker stop "$FE_NAME" >"$DIR/stop-fe.stdout" 2>"$DIR/stop-fe.stderr" || true
docker rm -f "$BE_NAME" >"$DIR/rm-be.stdout" 2>"$DIR/rm-be.stderr" || true
docker rm -f "$FE_NAME" >"$DIR/rm-fe.stdout" 2>"$DIR/rm-fe.stderr" || true
docker network rm "$NETWORK" >"$DIR/rm-net.stdout" 2>"$DIR/rm-net.stderr" || true

{
  echo "after"
  date -u +%Y-%m-%dT%H:%M:%SZ
  docker ps -a --filter "name=oo-dl01w1-12" --format '{{.ID}} {{.Names}} {{.Status}}'
  echo "remaining_count=$(docker ps -a --filter "name=oo-dl01w1-12" -q | wc -l | tr -d ' ')"
} >"$DIR/after.txt"
echo "cleanup recorded"
