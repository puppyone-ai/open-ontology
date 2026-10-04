#!/bin/bash
# One owned Linux arm64 Rust 1.94 builder. No host cargo cache, no host network.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a169
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP06/pg-policy-compiler
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a169-rust194
NET=oo-a169-net
CARGO_VOL=oo-a169-cargo
TARGET_VOL=oo-a169-target

mkdir -p "$EVIDENCE/linux/phases"
chmod 755 "$EVIDENCE/linux/inside.sh" "$EVIDENCE/linux/launch.sh"

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$EVIDENCE/linux/host-cargo-procs.txt" || true
host_n=$(grep -c . "$EVIDENCE/linux/host-cargo-procs.txt" || true)
docker ps --filter label=oo.kind=rust-builder --format '{{.Names}}' > "$EVIDENCE/linux/builders-before.txt" || true
builder_n=$(grep -c . "$EVIDENCE/linux/builders-before.txt" || true)
global_n=$((host_n + builder_n))
echo "host_cargo=$host_n rust_builders=$builder_n global=$global_n" | tee "$EVIDENCE/linux/capacity.txt"
if [ "$global_n" -ge 2 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$EVIDENCE/linux/launch.exit"
  exit 3
fi
own=$(docker ps -a --filter label=oo.owner=a169 --filter label=oo.kind=rust-builder --format '{{.Names}}')
if [ -n "$own" ]; then
  printf '%s\n' "$own" > "$EVIDENCE/linux/deferred-own.txt"
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$EVIDENCE/linux/launch.exit"
  exit 3
fi

docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$EVIDENCE/linux/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/linux/image.txt"

if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a169 \
    --label oo.session="${RUN_ID}" \
    --label oo.kind=task-net \
    "$NET" > "$EVIDENCE/linux/network.txt"
fi
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a169 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=rust-builder \
  "$CARGO_VOL" > "$EVIDENCE/linux/volume-cargo.txt"
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a169 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=rust-builder \
  "$TARGET_VOL" > "$EVIDENCE/linux/volume-target.txt"

cat > "$EVIDENCE/linux/invocation.txt" <<EOF
run_id=${RUN_ID}
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
msrv_override=RUSTUP_TOOLCHAIN=1.94.0
network=${NET}
container=${NAME}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
privileged=no
network_mode=bridge_not_host
home_mount=no
socket_mount=no
host_port=none
shared_cargo_cache=no
pull=never
EOF

docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --memory 8g \
  --cpus 3 \
  --pids-limit 2048 \
  --network "$NET" \
  --tmpfs /tmp:rw,nosuid,size=1g \
  -e HOME=/tmp \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  --mount "type=bind,source=${REPO},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a169 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=rust-builder \
  --label oo.task=pg-policy-compiler \
  --label oo.scope=sp06-pg-policy-compiler \
  "$IMAGE" \
  bash /src/evidence/${RUN_ID}/SP06/pg-policy-compiler/linux/inside.sh
status=$?
printf '%s\n' "$status" > "$EVIDENCE/linux/docker-run.exit"
docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}}' > "$EVIDENCE/linux/inspect.txt" || true
exit "$status"
