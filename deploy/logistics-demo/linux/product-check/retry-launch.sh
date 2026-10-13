#!/bin/bash
# Second offline pass. Reuses the private cargo and rustup volumes.
# Does not reinstall Clippy and does not reseed the registry.
# Writes a new evidence directory so the first failed pass stays intact.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a419/SP11/logistics-product-loop/linux/product-check-2
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
HERE=$(cd "$(dirname "$0")" && pwd)
CARGO_VOL=oo-a419-eef831aa-cargo
TARGET_VOL=oo-a419-eef831aa-target
RUSTUP_VOL=oo-a419-eef831aa-rustup
NAME=oo-a419-product-offline-2
mkdir -p "$EVIDENCE/phases"
chmod 755 "$HERE/offline-inside.sh" "$HERE/retry-launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite product-check-2 console.log" >&2
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
  docker volume inspect "$vol" >/dev/null 2>&1 || { echo "missing volume $vol" >&2; printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"; exit 1; }
done
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
set +e
docker run --name "$NAME" \
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
  --network none \
  -e HOME=/tmp \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${SRC},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${HERE}/offline-inside.sh,target=/inside.sh,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --mount "type=volume,source=${RUSTUP_VOL},target=/rustup" \
  --tmpfs /tmp:rw,nosuid,nodev,size=512m \
  --label oo.project=open-ontology \
  --label oo.owner=a419 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a419 \
  --label oo.task=logistics-product-check-2 \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
offline_exit=$?
printf '%s\n' "$offline_exit" > "$EVIDENCE/phases/offline-docker.exit"
docker inspect -f 'ExitCode={{.State.ExitCode}} OOMKilled={{.State.OOMKilled}} CapDrop={{json .HostConfig.CapDrop}} Memory={{.HostConfig.Memory}} NanoCpus={{.HostConfig.NanoCpus}} Network={{.HostConfig.NetworkMode}}' "$NAME" > "$EVIDENCE/offline-hostconfig.txt" 2>&1 || true
docker rm "$NAME" >/dev/null 2>&1 || true
printf '%s\n' "$offline_exit" > "$EVIDENCE/docker-run.exit"
exit "$offline_exit"
