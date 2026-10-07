#!/bin/bash
# One short-lived Linux arm64 Rust 1.94 builder. No PostgreSQL. No overlay.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a300/SP11/ontology-dialogue
SMOKE=${EVIDENCE}/linux/smoke
ROOT=/tmp/oo-a300-eef831aa-dba4-4af2-a78b-541bc9e0a300
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
FIXED=96afe70fd68a56ae8eec7504bf20e09ecd3cdf65
NAME=oo-a300-smoke
NET=oo-a300-eef831aa-net
CARGO_VOL=oo-a300-eef831aa-cargo
TARGET_VOL=oo-a300-eef831aa-target

mkdir -p "$SMOKE/phases" "$ROOT"
chmod 755 "$SMOKE/inside.sh" "$SMOKE/launch.sh"
if [ -f "$SMOKE/console.log" ]; then
  echo "refusing to overwrite smoke console.log" >&2
  printf '%s\n' 2 > "$SMOKE/launch.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$SMOKE/launch.exit"
  exit 1
fi
docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$SMOKE/image.txt"
grep -q 'arch=arm64' "$SMOKE/image.txt"

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$SMOKE/host-cargo-procs.txt" || true
host_n=$(grep -c . "$SMOKE/host-cargo-procs.txt" || true)
{
  docker ps --filter status=running --filter label=oo.kind=rust-builder --format '{{.Names}}'
  docker ps --filter status=running --filter ancestor=sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 --format '{{.Names}}'
} | awk 'NF && !seen[$0]++' > "$SMOKE/builders-before.txt" || true
builder_n=$(grep -c . "$SMOKE/builders-before.txt" || true)
global_n=$((host_n + builder_n))
printf 'host_cargo=%s rust_builders=%s global=%s cap=10\n' "$host_n" "$builder_n" "$global_n" > "$SMOKE/capacity.txt"
if [ "$global_n" -ge 10 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$SMOKE/launch.exit"
  exit 3
fi
own=$(docker ps -aq --filter status=running --filter label=oo.owner=a300 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$SMOKE/launch.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named smoke container already exists" >&2
  printf '%s\n' 2 > "$SMOKE/launch.exit"
  exit 2
fi
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a300 \
    --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a300 \
    --label oo.kind=network \
    "$NET" > "$SMOKE/network.txt"
fi
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a300 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a300 \
  --label oo.kind=cargo \
  "$CARGO_VOL" > "$SMOKE/volume-cargo.txt"
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a300 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a300 \
  --label oo.kind=target \
  "$TARGET_VOL" > "$SMOKE/volume-target.txt"

rm -rf "$ROOT/smoke-src"
mkdir -p "$ROOT/smoke-src"
git -C "$SRC" archive "$FIXED" tools/answer-citations rust-toolchain.toml | tar -x -C "$ROOT/smoke-src"
if [ -d "$ROOT/smoke-src/tools/ontology-dialogue" ]; then
  echo "archive contains ontology-dialogue" >&2
  printf '%s\n' 2 > "$SMOKE/launch.exit"
  exit 2
fi
printf '%s\n' "$FIXED" > "$SMOKE/fixed-base.txt"
git -C "$SRC" rev-parse "$FIXED^{commit}" > "$SMOKE/archive-commit.txt"

cat > "$SMOKE/invocation.txt" <<EOF
run_id=eef831aa-dba4-4af2-a78b-541bc9e0a300
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
network=${NET}
container=${NAME}
privileged=no
network_mode=bridge_not_host
home_mount=no
socket_mount=no
host_port=none
shared_cargo_cache=no
pull=never
memory=6g
cpus=1
phase=smoke
fixed=${FIXED}
EOF

docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --network "$NET" \
  --cap-drop ALL \
  --security-opt no-new-privileges:true \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 256 \
  --label oo.project=open-ontology \
  --label oo.owner=a300 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a300 \
  --label oo.kind=rust-builder \
  --label oo.task=ontology-dialogue-smoke \
  --mount "type=bind,source=${ROOT}/smoke-src,target=/src,readonly" \
  --mount "type=bind,source=${SMOKE},target=/evidence/linux/smoke" \
  --mount "type=bind,source=${SMOKE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  "$IMAGE" \
  bash /inside.sh > "$SMOKE/console.log" 2>&1
status=$?
printf '%s\n' "$status" > "$SMOKE/launch.exit"
docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} memory={{.HostConfig.Memory}}' > "$SMOKE/inspect.txt" || true
docker rm "$NAME" > "$SMOKE/removed-container.txt" 2>&1 || true
exit "$status"
