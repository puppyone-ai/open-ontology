#!/bin/bash
# Remove only this task's containers, network, volumes, and tmp directory.
# Does not prune. Does not remove pinned images. Does not touch foreign resources.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a273/SP11/answer-citations-review
LOG=${EVIDENCE}/cleanup/run.log
ROOT=/tmp/oo-a273-eef831aa-dba4-4af2-a78b-541bc9e0a273
if [ -f "$LOG" ]; then
  echo "refusing to overwrite cleanup/run.log" >&2
  exit 2
fi
exec >"$LOG" 2>&1
echo "owner=a273"
echo "session=eef831aa-dba4-4af2-a78b-541bc9e0a273"
echo "=== owned containers before ==="
docker ps -a --filter label=oo.owner=a273 --format '{{.Names}} {{.Status}}'
echo "list_exit=$?"
for name in oo-a273-smoke oo-a273-proof oo-a273-proof2 oo-a273-hash oo-a273-hash2; do
  if docker container inspect "$name" >/dev/null 2>&1; then
    docker rm -f "$name"
    echo "rm_${name}_exit=$?"
  else
    echo "absent_${name}"
  fi
done
if docker network inspect oo-a273-eef831aa-net >/dev/null 2>&1; then
  docker network rm oo-a273-eef831aa-net
  echo "network_rm_exit=$?"
else
  echo "network_absent"
fi
if docker volume inspect oo-a273-eef831aa-cargo >/dev/null 2>&1; then
  docker volume rm oo-a273-eef831aa-cargo
  echo "cargo_vol_rm_exit=$?"
else
  echo "cargo_vol_absent"
fi
if docker volume inspect oo-a273-eef831aa-target >/dev/null 2>&1; then
  docker volume rm oo-a273-eef831aa-target
  echo "target_vol_rm_exit=$?"
else
  echo "target_vol_absent"
fi
if [ -d "$ROOT" ]; then
  rm -rf "$ROOT"
  echo "tmp_rm_exit=$?"
else
  echo "tmp_absent"
fi
if [ -d "$ROOT" ]; then
  echo "tmp_still_present"
else
  echo "tmp_absent"
fi
docker image inspect docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 >/dev/null
echo "rust_image_still_exit=$?"
docker image inspect docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb >/dev/null
echo "postgres_image_still_exit=$?"
echo "=== owned containers after ==="
docker ps -a --filter label=oo.owner=a273 --format '{{.Names}} {{.Status}}'
echo "after_list_exit=$?"
