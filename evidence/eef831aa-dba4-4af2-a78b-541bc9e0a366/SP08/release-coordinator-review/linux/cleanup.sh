#!/bin/bash
# Remove only a366 resources. Does not prune and does not touch other owners.
set -u
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a366
EVID=/Users/redux/puppy-one/open-ontology/evidence/${RUN_ID}/SP08/release-coordinator-review
{
  echo "before"
  docker ps -a --filter label=oo.owner=a366 --format '{{.Names}} {{.Status}}' || true
  docker rm -f oo-a366-status oo-a366-build oo-a366-smoke oo-a366-pg >/dev/null 2>&1 || true
  docker volume rm oo-a366-eef831aa-pgdata oo-a366-cargo oo-a366-target >/dev/null 2>&1 || true
  docker network rm oo-a366-eef831aa-net >/dev/null 2>&1 || true
  rm -rf "/tmp/oo-a366-${RUN_ID}"
  echo "after"
  docker ps -a --filter label=oo.owner=a366 --format '{{.Names}} {{.Status}}' || true
  docker volume ls --filter label=oo.owner=a366 --format '{{.Name}}' || true
  docker network ls --filter label=oo.owner=a366 --format '{{.Name}}' || true
  if [ -d "/tmp/oo-a366-${RUN_ID}" ]; then
    echo "tmp_still_present"
  else
    echo "tmp_removed"
  fi
} > "$EVID/cleanup-owned.txt" 2>&1
printf '%s\n' 0 > "$EVID/cleanup.exit"
