#!/bin/bash
# Private arm64 builder. Clippy is installed once, then checks run with network none.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a419/SP11/logistics-product-loop/linux/product-check
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
HERE=$(cd "$(dirname "$0")" && pwd)
CARGO_VOL=oo-a419-eef831aa-cargo
TARGET_VOL=oo-a419-eef831aa-target
RUSTUP_VOL=oo-a419-eef831aa-rustup
FROM_CARGO=oo-a416-cargo
NET=oo-a419-eef831aa-build
INSTALL_NAME=oo-a419-clippy-install
OFFLINE_NAME=oo-a419-product-offline
mkdir -p "$EVIDENCE/phases" "$EVIDENCE/install" "$EVIDENCE/offline"
chmod 755 "$HERE/install-inside.sh" "$HERE/offline-inside.sh" "$HERE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite product-check console.log" >&2
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt" || { echo "image is not arm64" >&2; printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"; exit 1; }
for vol in "$CARGO_VOL" "$TARGET_VOL" "$RUSTUP_VOL"; do
  docker volume inspect "$vol" >/dev/null 2>&1 || docker volume create \
    --label oo.project=open-ontology \
    --label oo.owner=a419 \
    --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a419 \
    --label oo.kind=volume \
    "$vol" >/dev/null
done
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a419 \
    --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a419 \
    --label oo.kind=bridge \
    "$NET" >/dev/null
fi
own=$(docker ps -q --filter label=oo.owner=a419 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
seed() {
  docker run --rm --name oo-a419-volume-seed \
    --platform linux/arm64 \
    --pull never \
    --network none \
    --user 0:0 \
    --security-opt no-new-privileges \
    --cap-drop ALL \
    --memory 1g \
    --cpus 1 \
    --pids-limit 128 \
    --mount "type=volume,source=${FROM_CARGO},target=/from,readonly" \
    --mount "type=volume,source=${CARGO_VOL},target=/cargo" \
    --label oo.project=open-ontology \
    --label oo.owner=a419 \
    --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a419 \
    --label oo.kind=volume-seed \
    "$IMAGE" \
    bash -c 'if [ ! -d /cargo/registry ]; then cp -a /from/. /cargo/; fi; test -d /cargo/registry'
}
if ! seed > "$EVIDENCE/seed.log" 2>&1; then
  echo "cargo volume seed failed" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
printf '%s\n' 0 > "$EVIDENCE/phases/seed.exit"
run_builder() {
  name=$1
  network=$2
  script=$3
  evidence=$4
  docker run --name "$name" \
    --platform linux/arm64 \
    --pull never \
    --init \
    --user 0:0 \
    --security-opt no-new-privileges \
    --cap-drop ALL \
    --memory 6g \
    --memory-swap 6g \
    --cpus 1 \
    --pids-limit 256 \
    --network "$network" \
    -e HOME=/tmp \
    -e CARGO_TERM_COLOR=never \
    --mount "type=bind,source=${SRC},target=/src,readonly" \
    --mount "type=bind,source=${evidence},target=/evidence" \
    --mount "type=bind,source=${script},target=/inside.sh,readonly" \
    --mount "type=volume,source=${CARGO_VOL},target=/cargo" \
    --mount "type=volume,source=${TARGET_VOL},target=/target" \
    --mount "type=volume,source=${RUSTUP_VOL},target=/rustup" \
    --tmpfs /tmp:rw,nosuid,nodev,size=512m \
    --label oo.project=open-ontology \
    --label oo.owner=a419 \
    --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a419 \
    --label oo.task=logistics-product-check \
    --label oo.toolchain=1.94.0 \
    --label oo.platform=linux/arm64 \
    --label oo.kind=rust-builder \
    "$IMAGE" \
    bash /inside.sh
}
set +e
run_builder "$INSTALL_NAME" "$NET" "$HERE/install-inside.sh" "$EVIDENCE/install" > "$EVIDENCE/install-console.log" 2>&1
install_exit=$?
printf '%s\n' "$install_exit" > "$EVIDENCE/phases/install-docker.exit"
docker inspect -f 'ExitCode={{.State.ExitCode}} OOMKilled={{.State.OOMKilled}} CapDrop={{json .HostConfig.CapDrop}} Memory={{.HostConfig.Memory}} NanoCpus={{.HostConfig.NanoCpus}} Network={{.HostConfig.NetworkMode}}' "$INSTALL_NAME" > "$EVIDENCE/install-hostconfig.txt" 2>&1 || true
docker rm "$INSTALL_NAME" >/dev/null 2>&1 || true
if [ "$install_exit" -ne 0 ]; then
  printf '%s\n' "$install_exit" > "$EVIDENCE/docker-run.exit"
  exit "$install_exit"
fi
run_builder "$OFFLINE_NAME" none "$HERE/offline-inside.sh" "$EVIDENCE/offline" > "$EVIDENCE/console.log" 2>&1
offline_exit=$?
printf '%s\n' "$offline_exit" > "$EVIDENCE/phases/offline-docker.exit"
docker inspect -f 'ExitCode={{.State.ExitCode}} OOMKilled={{.State.OOMKilled}} CapDrop={{json .HostConfig.CapDrop}} Memory={{.HostConfig.Memory}} NanoCpus={{.HostConfig.NanoCpus}} Network={{.HostConfig.NetworkMode}}' "$OFFLINE_NAME" > "$EVIDENCE/offline-hostconfig.txt" 2>&1 || true
docker rm "$OFFLINE_NAME" >/dev/null 2>&1 || true
printf '%s\n' "$offline_exit" > "$EVIDENCE/docker-run.exit"
exit "$offline_exit"
