#!/bin/bash
# Removes only this card's containers, volumes, network, and /tmp directory.
set -eu
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a384
RAW=/Users/redux/puppy-one/open-ontology/evidence/${RUN_ID}/SP02/secret-control-api/linux/raw
mkdir -p "$RAW"
docker rm -f oo-a384-proof >/dev/null 2>&1 || true
docker rm -f oo-a384-pg >/dev/null 2>&1 || true
docker volume rm oo-a384-eef831aa-pgdata >/dev/null 2>&1 || true
docker volume rm oo-a384-cargo >/dev/null 2>&1 || true
docker volume rm oo-a384-work >/dev/null 2>&1 || true
docker network rm oo-a384-eef831aa-net >/dev/null 2>&1 || true
rm -rf "/tmp/oo-a384-${RUN_ID}"
{
  echo "removed=oo-a384-proof oo-a384-pg oo-a384-eef831aa-pgdata oo-a384-cargo oo-a384-work oo-a384-eef831aa-net /tmp/oo-a384-${RUN_ID}"
  echo "foreign_cleanup=no"
  echo "prune=no"
} > "$RAW/cleanup.txt"
echo "cleanup_ok"
