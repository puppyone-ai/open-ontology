#!/bin/bash
# One rust-builder for the stage CLIs. Refuses a second builder or a live host toolchain.
set -u
SRC=/Users/redux/puppy-one/open-ontology
HERE=$(cd "$(dirname "$0")" && pwd)
EVIDENCE=${SRC}/evidence/gpt-minimal-closure/stages
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-gpt-closure-stages
CARGO_VOL=oo-a419-eef831aa-cargo
TARGET_VOL=oo-a419-eef831aa-target
RUSTUP_VOL=oo-a419-eef831aa-rustup
mkdir -p "$EVIDENCE/phases"
chmod 755 "$HERE/stages-inside.sh"
own=$(docker ps -q --filter label=oo.owner=a419 --filter label=oo.kind=rust-builder | wc -l | tr -d ' ')
global=$(docker ps -q --filter label=oo.kind=rust-builder | wc -l | tr -d ' ')
host=0
for name in cargo rustc rustdoc rustup; do
  count=$(pgrep -x "$name" | wc -l | tr -d ' ')
  host=$((host + count))
done
printf 'own=%s global=%s host_rust_pids=%s\n' "$own" "$global" "$host" | tee "$EVIDENCE/capacity.txt"
if [ "$own" != "0" ] || [ "$global" -ge 10 ] || [ "$host" != "0" ]; then
  echo "capacity refused" >&2
  exit 2
fi
if docker ps -a --format '{{.Names}}' | grep -qx "$NAME"; then
  echo "container name already exists" >&2
  exit 2
fi
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
  --mount "type=bind,source=${HERE}/stages-inside.sh,target=/inside.sh,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --mount "type=volume,source=${RUSTUP_VOL},target=/rustup" \
  --tmpfs /tmp:rw,nosuid,nodev,size=512m \
  --label oo.project=open-ontology \
  --label oo.owner=a419 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a419 \
  --label oo.task=logistics-closure-stages \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh
code=$?
docker inspect -f 'ExitCode={{.State.ExitCode}} OOMKilled={{.State.OOMKilled}} CapDrop={{json .HostConfig.CapDrop}} Memory={{.HostConfig.Memory}} NanoCpus={{.HostConfig.NanoCpus}} Network={{.HostConfig.NetworkMode}}' "$NAME" > "$EVIDENCE/hostconfig.txt" 2>&1 || true
printf '%s\n' "$code" > "$EVIDENCE/docker-run.exit"
exit "$code"
