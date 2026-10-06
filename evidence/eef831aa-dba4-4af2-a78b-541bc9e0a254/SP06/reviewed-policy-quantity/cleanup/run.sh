#!/bin/bash
# Remove only a254 containers, volumes, network, and the proof tmp.
# Does not prune. Does not remove pinned images. Does not touch other owners.
set -u
echo "before containers:"
docker ps -a --filter name=oo-a254 --format '{{.Names}} {{.Status}}'
echo "before volumes:"
docker volume ls --filter name=oo-a254 --format '{{.Name}}'
echo "before networks:"
docker network ls --filter name=oo-a254 --format '{{.Name}}'

docker rm -f oo-a254-pg
docker volume rm oo-a254-eef831aa-cargo oo-a254-eef831aa-pgdata oo-a254-eef831aa-target
docker network rm oo-a254-eef831aa-net
rm -rf /tmp/oo-a254-eef831aa-dba4-4af2-a78b-541bc9e0a254

echo "after containers:"
docker ps -a --filter name=oo-a254 --format '{{.Names}} {{.Status}}'
echo "after volumes:"
docker volume ls --filter name=oo-a254 --format '{{.Name}}'
echo "after networks:"
docker network ls --filter name=oo-a254 --format '{{.Name}}'
echo "tmp_exists=$(test -d /tmp/oo-a254-eef831aa-dba4-4af2-a78b-541bc9e0a254 && echo yes || echo no)"
