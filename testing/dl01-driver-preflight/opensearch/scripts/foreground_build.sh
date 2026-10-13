#!/bin/bash
# Admit one Rust slot, start oo-dl01w1-44-rust, and wait until that container exits.
# A launcher return is not a test result. The container exit is the phase result.
set -u
ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/testing/dl01-driver-preflight/opensearch"
EVIDENCE="$ROOT/evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-driver-preflight"
mkdir -p "$EVIDENCE/build"

if docker ps --format '{{.Names}}' | grep -qx 'oo-dl01w1-44-rust'; then
  echo "ACTIVE_BUILDER name=oo-dl01w1-44-rust"
  docker inspect -f 'status={{.State.Status}} started={{.State.StartedAt}}' oo-dl01w1-44-rust
  exit 1
fi

echo "foreground_start $(date -u +%Y-%m-%dT%H:%M:%SZ)"
set +e
bash "$SRC/scripts/start_build.sh"
start_code=$?
set -e
if [ "$start_code" != "0" ]; then
  echo "start_exit=${start_code} capacity_wait_900_begin $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  sleep 900
  echo "capacity_wait_900_end $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  if docker ps --format '{{.Names}}' | grep -qx 'oo-dl01w1-44-rust'; then
    echo "ACTIVE_BUILDER appeared during the capacity wait"
    exit 1
  fi
  set +e
  bash "$SRC/scripts/start_build.sh"
  start_code=$?
  set -e
  if [ "$start_code" != "0" ]; then
    echo "start_exit=${start_code} after one 900s capacity wait"
    exit "$start_code"
  fi
fi

echo "CONTAINER_STARTED $(date -u +%Y-%m-%dT%H:%M:%SZ)"
docker inspect -f 'status={{.State.Status}} id={{.Id}}' oo-dl01w1-44-rust
container_exit="$(docker wait oo-dl01w1-44-rust | tail -n 1)"
echo "DOCKER_PHASE=exited container_exit=${container_exit} $(date -u +%Y-%m-%dT%H:%M:%SZ)"
docker inspect -f 'status={{.State.Status}} exit={{.State.ExitCode}} started={{.State.StartedAt}} finished={{.State.FinishedAt}} oom={{.State.OOMKilled}} error={{.State.Error}}' oo-dl01w1-44-rust
for name in build.exit driver-build.exit test.exit clippy.exit preview.exit preview-test.exit preview-clippy.exit lock.exit; do
  if [ -f "$EVIDENCE/build/$name" ]; then
    printf '%s=' "$name"
    cat "$EVIDENCE/build/$name"
  else
    echo "${name}=MISSING"
  fi
done

docker logs oo-dl01w1-44-rust > "$EVIDENCE/build/container.stdout" 2> "$EVIDENCE/build/container.stderr" || true
owner="$(docker inspect -f '{{index .Config.Labels "owner"}}' oo-dl01w1-44-rust)"
status="$(docker inspect -f '{{.State.Status}}' oo-dl01w1-44-rust)"
if [ "$owner" = "oo-dl01w1-44" ] && [ "$status" != "running" ]; then
  docker rm oo-dl01w1-44-rust >/dev/null
  python3 "$SRC/scripts/admit.py" release rust >/dev/null
  echo "RELEASED_RUST"
fi
exit "$container_exit"
