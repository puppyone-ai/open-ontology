#!/bin/bash
# Remove only a259 containers, volumes, network, and the proof tmp.
# Exact names. Does not prune. Does not remove pinned images. Does not touch other owners.
set -u
echo "before containers:"
docker ps -a --filter name=oo-a259- --format '{{.Names}} {{.Status}}'
echo "before volumes:"
docker volume ls --filter name=oo-a259-eef831aa --format '{{.Name}}'
echo "before networks:"
docker network ls --filter name=oo-a259-eef831aa-net --format '{{.Name}}'

docker rm -f oo-a259-pg oo-a259-review oo-a259-hash oo-a259-hash-2
docker volume rm oo-a259-eef831aa-cargo oo-a259-eef831aa-pgdata oo-a259-eef831aa-target
docker network rm oo-a259-eef831aa-net
rm -rf /tmp/oo-a259-eef831aa-dba4-4af2-a78b-541bc9e0a259

echo "after containers:"
docker ps -a --filter name=oo-a259- --format '{{.Names}} {{.Status}}'
echo "after volumes:"
docker volume ls --filter name=oo-a259-eef831aa --format '{{.Name}}'
echo "after networks:"
docker network ls --filter name=oo-a259-eef831aa-net --format '{{.Name}}'
echo "tmp_exists=$(test -d /tmp/oo-a259-eef831aa-dba4-4af2-a78b-541bc9e0a259 && echo yes || echo no)"
