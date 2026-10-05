#!/bin/bash
# Fourth builder. Reuses the a201 network and volumes. Does not remove oo-a201-red2.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a201/SP06/parser-reap-bounds
SMOKE=${EVIDENCE}/linux/smoke
ROOT=/tmp/oo-a201-eef831aa-dba4-4af2-a78b-541bc9e0a201
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a201-red4
NET=oo-a201-eef831aa-net
CARGO_VOL=oo-a201-eef831aa-cargo
TARGET_VOL=oo-a201-eef831aa-target
OUT=${SMOKE}/red-attempt-4
mkdir -p "$OUT"
if [ -f "$OUT/console.log" ]; then
  echo "refusing to overwrite red-attempt-4 console" >&2
  exit 2
fi
ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$OUT/host-cargo-procs.txt" || true
host_n=$(grep -c . "$OUT/host-cargo-procs.txt" || true)
{
  docker ps --filter status=running --filter label=oo.kind=rust-builder --format '{{.Names}}'
  docker ps --filter status=running --filter ancestor=sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 --format '{{.Names}}'
} | awk 'NF && !seen[$0]++' > "$OUT/builders-before.txt" || true
builder_n=$(grep -c . "$OUT/builders-before.txt" || true)
global_n=$((host_n + builder_n))
printf 'host_cargo=%s rust_builders=%s global=%s cap=6\n' "$host_n" "$builder_n" "$global_n" > "$OUT/capacity.txt"
if [ "$global_n" -ge 6 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  exit 3
fi
own=$(docker ps -aq --filter status=running --filter label=oo.owner=a201 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named container already exists" >&2
  exit 2
fi

set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --memory 8g \
  --memory-swap 8g \
  --cpus 4 \
  --pids-limit 4096 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${ROOT}/archive,target=/src" \
  --mount "type=bind,source=${SMOKE}/red4-inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a201 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a201 \
  --label oo.task=parser-reap-bounds-red4 \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$OUT/console.log" 2>&1 &
docker_pid=$!
run_exit=124
for _ in $(seq 1 96); do
  if ! kill -0 "$docker_pid" 2>/dev/null; then
    wait "$docker_pid"
    run_exit=$?
    break
  fi
  sleep 5
done
if kill -0 "$docker_pid" 2>/dev/null; then
  echo "host_watchdog_killed=1" >> "$OUT/console.log"
  docker kill "$NAME" >/dev/null 2>&1 || true
  wait "$docker_pid"
  run_exit=124
fi
printf '%s\n' "$run_exit" > "$OUT/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME" 2>/dev/null || echo missing)"
  echo "PidsLimit=$(docker inspect -f '{{.HostConfig.PidsLimit}}' "$NAME" 2>/dev/null || echo missing)"
} > "$OUT/hostconfig.txt"
docker rm "$NAME" >/dev/null 2>&1
printf '%s\n' removed > "$OUT/container-removed.txt"
exit "$run_exit"
