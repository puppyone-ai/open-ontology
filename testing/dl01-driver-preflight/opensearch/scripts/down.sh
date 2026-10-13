#!/bin/bash
# Remove this task's runtime only. The image and cargo caches stay.
set -u
ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/testing/dl01-driver-preflight/opensearch"
if docker ps --format '{{.Names}}' | grep -qx 'oo-dl01w1-44-rust'; then
  echo "rust builder is running; refusing cleanup" >&2
  exit 1
fi
for name in oo-dl01w1-44-runner oo-dl01w1-44-opensearch oo-dl01w1-44-hash; do
  if docker ps -a --format '{{.Names}}' | grep -qx "$name"; then
    owner="$(docker inspect -f '{{index .Config.Labels "owner"}}' "$name")"
    if [ "$owner" != "oo-dl01w1-44" ]; then
      echo "refusing to remove foreign container $name" >&2
      exit 1
    fi
    docker rm -f "$name" >/dev/null
  fi
done
docker network rm oo-dl01w1-44-net >/dev/null 2>&1 || true
docker volume rm oo-dl01w1-44-data >/dev/null 2>&1 || true
if [ -d /tmp/oo-dl01w1-44-private ]; then
  rm -rf /tmp/oo-dl01w1-44-private
fi
python3 "$SRC/scripts/admit.py" release all >/dev/null
echo "runtime_removed=1"
