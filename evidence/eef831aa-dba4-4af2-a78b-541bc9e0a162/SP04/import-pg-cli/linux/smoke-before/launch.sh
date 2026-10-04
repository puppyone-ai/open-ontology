#!/bin/bash
# Pre-edit Linux 1.94 smoke. One rust builder. No PostgreSQL. No product overlay.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
BASE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a162/SP04/import-pg-cli
EVIDENCE=${BASE}/linux/smoke-before
ROOT=/tmp/oo-a162-eef831aa-dba4-4af2-a78b-541bc9e0a162
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a162-smoke
NET=oo-a162-eef831aa-net
CARGO_VOL=oo-a162-eef831aa-cargo
TARGET_VOL=oo-a162-eef831aa-target
FIXED=cc764cbc5f9d5d2cf6056a2c41ff2612161b5ff4
SNAP=${ROOT}/smoke-src

mkdir -p "$EVIDENCE" "$ROOT/ca"
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite smoke console.log" >&2
  exit 2
fi
head_now=$(git -C "$SRC" rev-parse HEAD)
printf '%s\n' "$head_now" > "$EVIDENCE/workspace-head.txt"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  exit 1
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a162 \
    --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a162 \
    "$NET" > "$EVIDENCE/network.txt"
fi
docker network inspect --format '{{.Id}} {{.Driver}}' "$NET" > "$EVIDENCE/network.txt"
docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}' > "$EVIDENCE/containers-before.txt"
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a162 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s global_rust_builders=%s\n' "$running_rust" "$own" "$host_cargo" "$global_builders" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$((running_rust + host_cargo))" -ge 2 ] || [ "$global_builders" -ge 2 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
git -C "$SRC" diff --exit-code "$FIXED" -- crates/oo-pipeline crates/oo-connectors Cargo.lock
printf '%s\n' "$FIXED" > "$EVIDENCE/archive-base.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/fixed-tree.txt"
git -C "$SRC" rev-parse HEAD^{tree} > "$EVIDENCE/archive-tree.txt"
rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive HEAD | tar -x -C "$SNAP"
if [ -e "$SNAP/crates/oo-pipeline/src/import_cli.rs" ]; then
  echo "archive already contains the import command" >&2
  exit 2
fi
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a162 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a162 \
  --label oo.kind=cargo \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a162 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a162 \
  --label oo.kind=target \
  "$TARGET_VOL" >/dev/null
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named smoke container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
cat > "$EVIDENCE/invocation.txt" <<EOF
workspace_head=${head_now}
fixed=${FIXED}
composition=git archive HEAD, pipeline and connectors match fixed base, no overlay
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
container=${NAME}
network=${NET}
pull=never
privileged=no
home_mount=no
socket_mount=no
host_network=no
shared_cargo_cache=no
host_ca_mounted=no
private_ca=${ROOT}/ca
phases=toolchain fetch pipeline-lib connectors-lib pipeline-clippy connectors-clippy
EOF
set +e
docker run \
  --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 8g \
  --memory-swap 8g \
  --cpus 4 \
  --pids-limit 4096 \
  --network "$NET" \
  --workdir /src \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/tmp \
  --mount "type=bind,source=${SNAP},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${ROOT}/ca,target=/private-ca,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a162 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a162 \
  --label oo.task=import-pg-cli-smoke-before \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
  echo "CapAdd=$(docker inspect -f '{{json .HostConfig.CapAdd}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "PortBindings=$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$EVIDENCE/hostconfig.txt"
exit "$run_exit"
