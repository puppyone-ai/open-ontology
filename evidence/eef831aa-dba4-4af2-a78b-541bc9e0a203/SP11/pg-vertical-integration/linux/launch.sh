#!/bin/bash
# One owned Linux arm64 Rust 1.94 builder for the pre-edit pipeline and gateway smoke.
# Mounts the immutable fixed archive, not the moving workspace.
# No host cargo cache, no host network, no second builder for this task.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a203
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP11/pg-vertical-integration
SRC=/tmp/oo-a203-fixed
FIXED=0ee7cf86ad443ee1d84fe9fe3ed3f7d8847799c1
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a203-rust194
NET=oo-a203-net
CARGO_VOL=oo-a203-cargo
TARGET_VOL=oo-a203-target

mkdir -p "$EVIDENCE/linux/phases"
chmod 755 "$EVIDENCE/linux/inside.sh" "$EVIDENCE/linux/launch.sh" "$EVIDENCE/linux/libclang-extract.sh"

if [ ! -f "$SRC/Cargo.lock" ] || [ ! -f "$SRC/crates/oo-pipeline/src/full_import.rs" ]; then
  rm -rf "$SRC"
  mkdir -p "$SRC"
  git -C "$REPO" archive --format=tar "$FIXED" \
    Cargo.toml Cargo.lock rust-toolchain.toml crates contracts \
    | tar -x -C "$SRC"
  git -C "$REPO" rev-parse "${FIXED}^{commit}" > "$SRC/.fixed-base-sha"
fi
if [ ! -f "$SRC/contracts/pipeline/full-import-v1.json" ]; then
  git -C "$REPO" archive --format=tar "$FIXED" contracts | tar -x -C "$SRC"
fi
if [ ! -f "$SRC/Cargo.lock" ] || [ ! -f "$SRC/crates/oo-gateway/src/policy.rs" ]; then
  echo "fixed archive is missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/linux/launch.exit"
  exit 2
fi
if [ -f "$SRC/crates/oo-gateway/src/bin/query-pg.rs" ]; then
  echo "fixed archive contains the moving query CLI" >&2
  printf '%s\n' 2 > "$EVIDENCE/linux/launch.exit"
  exit 2
fi
git -C "$REPO" rev-parse "${FIXED}^{commit}" > "$EVIDENCE/linux/fixed-base.txt"
cp "$SRC/.fixed-base-sha" "$EVIDENCE/linux/archive-sha.txt"

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$EVIDENCE/linux/host-cargo-procs.txt" || true
host_n=$(grep -c . "$EVIDENCE/linux/host-cargo-procs.txt" || true)
{
  docker ps --filter label=oo.kind=rust-builder --format '{{.Names}}'
  docker ps --filter ancestor=sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 --format '{{.Names}}'
} | awk 'NF && !seen[$0]++' > "$EVIDENCE/linux/builders-before.txt" || true
builder_n=$(grep -c . "$EVIDENCE/linux/builders-before.txt" || true)
global_n=$((host_n + builder_n))
echo "host_cargo=$host_n rust_builders=$builder_n global=$global_n cap=6" | tee "$EVIDENCE/linux/capacity.txt"
if [ "$global_n" -ge 6 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$EVIDENCE/linux/launch.exit"
  exit 3
fi
own=$(docker ps -a --filter label=oo.owner=a203 --filter label=oo.kind=rust-builder --format '{{.Names}}')
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
    --label oo.owner=a203 \
    --label oo.session="${RUN_ID}" \
    --label oo.kind=task-net \
    "$NET" > "$EVIDENCE/linux/network.txt"
fi
if ! docker volume inspect "$CARGO_VOL" >/dev/null 2>&1; then
  docker volume create \
    --label oo.project=open-ontology \
    --label oo.owner=a203 \
    --label oo.session="${RUN_ID}" \
    --label oo.kind=rust-builder \
    "$CARGO_VOL" > "$EVIDENCE/linux/volume-cargo.txt"
else
  docker volume inspect "$CARGO_VOL" --format '{{.Name}}' > "$EVIDENCE/linux/volume-cargo.txt"
fi
if ! docker volume inspect "$TARGET_VOL" >/dev/null 2>&1; then
  docker volume create \
    --label oo.project=open-ontology \
    --label oo.owner=a203 \
    --label oo.session="${RUN_ID}" \
    --label oo.kind=rust-builder \
    "$TARGET_VOL" > "$EVIDENCE/linux/volume-target.txt"
else
  docker volume inspect "$TARGET_VOL" --format '{{.Name}}' > "$EVIDENCE/linux/volume-target.txt"
fi

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
source_mount=${SRC}
source_commit=${FIXED}
privileged=no
network_mode=bridge_not_host
home_mount=no
socket_mount=no
host_port=none
shared_cargo_cache=no
pull=never
phase=preedit
packages=oo-pipeline,oo-gateway
targets=lib
global_cap=6
memory=8g
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
  --tmpfs /tmp:rw,nosuid,size=4g \
  -e HOME=/tmp \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  --mount "type=bind,source=${SRC},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a203 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=rust-builder \
  --label oo.task=pg-vertical-integration \
  --label oo.scope=sp11-pg-vertical \
  "$IMAGE" \
  bash /evidence/linux/inside.sh \
  > "$EVIDENCE/linux/docker-run.log" 2>&1
status=$?
printf '%s\n' "$status" > "$EVIDENCE/linux/docker-run.exit"
docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} capdrop={{json .HostConfig.CapDrop}}' > "$EVIDENCE/linux/inspect.txt" || true
printf '%s\n' "$status" > "$EVIDENCE/linux/launch.exit"
exit "$status"
