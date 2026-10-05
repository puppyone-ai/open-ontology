#!/bin/bash
# One Linux arm64 Rust 1.94 builder for the executor lib tests, clippy, and live PG test.
# Removes only this task's exited smoke container. Keeps cargo and target volumes.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a180
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP06/query-executor
SEC=/tmp/oo-a180-${RUN_ID}/secrets
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a180-rust194
NET=oo-a180-net
CARGO_VOL=oo-a180-cargo
TARGET_VOL=oo-a180-target

mkdir -p "$EVIDENCE/linux/live"
chmod 755 "$EVIDENCE/linux/run-live.sh" "$EVIDENCE/linux/launch-live.sh"

if docker inspect "$NAME" >/dev/null 2>&1; then
  state=$(docker inspect -f '{{.State.Status}}' "$NAME")
  if [ "$state" = "exited" ]; then
    docker rm "$NAME" > "$EVIDENCE/linux/live/removed-container.txt"
  fi
fi

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$EVIDENCE/linux/live/host-cargo-procs.txt" || true
host_n=$(grep -c . "$EVIDENCE/linux/live/host-cargo-procs.txt" || true)
{
  docker ps --filter label=oo.kind=rust-builder --format '{{.Names}}'
  docker ps --filter ancestor=sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 --format '{{.Names}}'
} | awk 'NF && !seen[$0]++' > "$EVIDENCE/linux/live/builders-before.txt" || true
builder_n=$(grep -c . "$EVIDENCE/linux/live/builders-before.txt" || true)
global_n=$((host_n + builder_n))
echo "host_cargo=$host_n rust_builders=$builder_n global=$global_n cap=3" | tee "$EVIDENCE/linux/live/capacity.txt"
if [ "$global_n" -ge 3 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$EVIDENCE/linux/live/launch.exit"
  exit 3
fi
own=$(docker ps -a --filter label=oo.owner=a180 --filter label=oo.kind=rust-builder --format '{{.Names}}')
if [ -n "$own" ]; then
  printf '%s\n' "$own" > "$EVIDENCE/linux/live/deferred-own.txt"
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$EVIDENCE/linux/live/launch.exit"
  exit 3
fi
if ! docker ps --filter name=^/oo-a180-pg$ --filter status=running --format '{{.Names}}' | grep -q .; then
  echo "postgres is not running" >&2
  printf '%s\n' 2 > "$EVIDENCE/linux/live/launch.exit"
  exit 2
fi

docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$EVIDENCE/linux/live/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/linux/live/image.txt"

cat > "$EVIDENCE/linux/live/invocation.txt" <<EOF
run_id=${RUN_ID}
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
msrv_override=RUSTUP_TOOLCHAIN=1.94.0
network=${NET}
container=${NAME}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
secrets_mount=/run/oo-a180/fixture
privileged=no
network_mode=bridge_not_host
home_mount=no
socket_mount=no
host_port=none
shared_cargo_cache=no
pull=never
phase=live
global_cap=3
postgres=oo-a180-pg
EOF

docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --memory 8g \
  --memory-swap 8g \
  --cpus 4 \
  --pids-limit 4096 \
  --network "$NET" \
  --tmpfs /tmp:rw,nosuid,size=1g \
  -e HOME=/tmp \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  --mount "type=bind,source=${REPO},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${SEC},target=/run/oo-a180/fixture,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a180 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=rust-builder \
  --label oo.task=query-executor \
  --label oo.scope=sp06-query-executor \
  "$IMAGE" \
  bash /src/evidence/${RUN_ID}/SP06/query-executor/linux/run-live.sh \
  > "$EVIDENCE/linux/live/docker-run.log" 2>&1
status=$?
printf '%s\n' "$status" > "$EVIDENCE/linux/live/docker-run.exit"
docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} capdrop={{json .HostConfig.CapDrop}}' > "$EVIDENCE/linux/live/inspect.txt" || true
printf '%s\n' "$status" > "$EVIDENCE/linux/live/launch.exit"
exit "$status"
