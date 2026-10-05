#!/bin/bash
# Last evidence writer. Stdout and inspect stay outside the evidence tree.
# The only files this run may create under the evidence root are SHA256SUMS and SHA256SUMS.sha256.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a201/SP06/parser-reap-bounds
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a201-hash-run
NET=oo-a201-eef831aa-net
CARGO_VOL=oo-a201-eef831aa-cargo
LOG=/tmp/oo-a201-hash-second.log
META=/tmp/oo-a201-hash-second-hostconfig.txt
if [ -e "$EVIDENCE/SHA256SUMS" ] || [ -e "$EVIDENCE/SHA256SUMS.sha256" ]; then
  echo "manifest already exists" >&2
  exit 2
fi
own=$(docker ps -aq --filter status=running --filter label=oo.owner=a201 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: rust builder still running" >&2
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
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a201 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a201 \
  --label oo.task=parser-reap-bounds-hash-run \
  --label oo.platform=linux/arm64 \
  --label oo.kind=hash-runner \
  "$IMAGE" \
  bash -c '/cargo-home/bin/oo-a201-sha256 dir /evidence/SHA256SUMS /evidence && echo STAMP && cat /evidence/SHA256SUMS.sha256 && echo REHASH && /cargo-home/bin/oo-a201-sha256 file /evidence/SHA256SUMS' \
  > "$LOG" 2>&1 &
docker_pid=$!
run_exit=124
for _ in $(seq 1 40); do
  if ! kill -0 "$docker_pid" 2>/dev/null; then
    wait "$docker_pid"
    run_exit=$?
    break
  fi
  sleep 3
done
if kill -0 "$docker_pid" 2>/dev/null; then
  echo "host_watchdog_killed=1" >> "$LOG"
  docker kill "$NAME" >/dev/null 2>&1 || true
  wait "$docker_pid"
  run_exit=124
fi
{
  echo "docker_run_exit=$run_exit"
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Image=$(docker inspect -f '{{.Image}}' "$NAME" 2>/dev/null || echo missing)"
} > "$META"
docker rm "$NAME" >/dev/null 2>&1
echo "second_log=$LOG"
echo "second_meta=$META"
exit "$run_exit"
