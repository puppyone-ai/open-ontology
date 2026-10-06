#!/bin/bash
# Remove only this task's containers, volumes, network, and private tmp.
# Does not prune. Does not remove pinned images or foreign resources.
set -u
NET=oo-a303-eef831aa-net
CARGO_VOL=oo-a303-eef831aa-cargo
TARGET_VOL=oo-a303-eef831aa-target
TMP=/tmp/oo-a303-eef831aa-dba4-4af2-a78b-541bc9e0a303
EVIDENCE=/Users/redux/puppy-one/open-ontology/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a303/SP04/cdc-quarantine-review
LOG=$EVIDENCE/cleanup.txt
if [ -f "$LOG" ]; then
  echo "refusing to overwrite cleanup.txt" >&2
  exit 2
fi
{
  echo "=== containers ==="
  docker ps -a --filter label=oo.owner=a303 --format '{{.Names}} {{.Status}}'
  for name in oo-a303-preoverlay oo-a303-overlay oo-a303-overlay2 oo-a303-overlay3 oo-a303-overlay4 oo-a303-manifest oo-a303-manifest-close; do
    if docker container inspect "$name" >/dev/null 2>&1; then
      docker rm -f "$name"
    else
      echo "absent container $name"
    fi
  done
  echo "=== volumes ==="
  docker volume rm "$CARGO_VOL" || true
  docker volume rm "$TARGET_VOL" || true
  echo "=== network ==="
  docker network rm "$NET" || true
  echo "=== tmp ==="
  rm -rf "$TMP"
  echo "removed $TMP"
  echo "=== left ==="
  docker ps -a --filter label=oo.owner=a303 --format '{{.Names}}' || true
  docker volume ls --filter name=oo-a303 --format '{{.Name}}' || true
  docker network ls --filter name=oo-a303 --format '{{.Name}}' || true
} > "$LOG" 2>&1
echo "cleanup recorded"
