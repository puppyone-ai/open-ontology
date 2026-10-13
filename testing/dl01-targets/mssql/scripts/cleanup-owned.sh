#!/bin/bash
# Remove only this task's container and volume. Refuses any other name.
set -euo pipefail
name="oo-dl01w1-09-mssql"
volume="oo-dl01w1-09-mssql"
case "$name" in
  oo-dl01w1-09-*) ;;
  *) echo "refusing name outside prefix" >&2; exit 64 ;;
esac
echo "before"
docker ps -a --filter "name=^/${name}$" --format '{{.ID}} {{.Names}} {{.Image}} {{.Status}}' || true
if docker ps -a --format '{{.Names}}' | grep -qx "$name"; then
  docker rm -f "$name"
else
  echo "no container named ${name}"
fi
if docker volume ls --format '{{.Name}}' | grep -qx "$volume"; then
  docker volume rm "$volume"
else
  echo "no volume named ${volume}"
fi
echo "after"
docker ps -a --filter "name=^/${name}$" --format '{{.ID}} {{.Names}} {{.Image}} {{.Status}}' || true
