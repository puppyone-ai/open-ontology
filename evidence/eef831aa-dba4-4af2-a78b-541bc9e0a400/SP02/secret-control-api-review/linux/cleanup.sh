#!/bin/bash
# Removes only this review's containers, volumes, network, and /tmp directory.
set -eu
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a400
RAW=/Users/redux/puppy-one/open-ontology/evidence/${RUN_ID}/SP02/secret-control-api-review/linux/raw
mkdir -p "$RAW"
docker rm -f oo-a400-proof >/dev/null 2>&1 || true
docker rm -f oo-a400-pg >/dev/null 2>&1 || true
docker rm -f oo-a400-hash >/dev/null 2>&1 || true
docker volume rm oo-a400-eef831aa-pgdata >/dev/null 2>&1 || true
docker volume rm oo-a400-cargo >/dev/null 2>&1 || true
docker volume rm oo-a400-work >/dev/null 2>&1 || true
docker network rm oo-a400-eef831aa-net >/dev/null 2>&1 || true
rm -rf "/tmp/oo-a400-${RUN_ID}"
{
  echo "removed=oo-a400-proof oo-a400-pg oo-a400-hash oo-a400-eef831aa-pgdata oo-a400-cargo oo-a400-work oo-a400-eef831aa-net /tmp/oo-a400-${RUN_ID}"
  echo "foreign_cleanup=no"
  echo "prune=no"
} > "$RAW/cleanup.txt"
echo "cleanup_ok"
