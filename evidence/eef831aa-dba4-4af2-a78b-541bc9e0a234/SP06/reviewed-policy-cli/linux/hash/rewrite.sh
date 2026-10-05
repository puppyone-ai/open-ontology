#!/bin/bash
# Second inventory pass. --rm. Writes only SHA256SUMS and SHA256SUMS.sha256. Inspect is not stored.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a234/SP06/reviewed-policy-cli
H=${EVIDENCE}/linux/hash
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a234-hash2

if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  exit 1
fi
host_n=$(ps -axo comm= | awk '$1 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {c++} END {print c+0}')
builder_n=$(docker ps --filter status=running --filter label=oo.kind=rust-builder --format '{{.Names}}' | awk 'NF' | wc -l | tr -d ' ')
global_n=$((host_n + builder_n))
if [ "$global_n" -ge 10 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named rewrite container already exists" >&2
  exit 2
fi

docker run --name "$NAME" --rm \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 256 \
  --network none \
  -e HOME=/tmp \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  --mount "type=bind,source=${H}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=64m \
  --tmpfs /work:rw,nosuid,nodev,exec,size=64m \
  --label oo.project=open-ontology \
  --label oo.owner=a234 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a234 \
  --label oo.task=reviewed-policy-cli-hash2 \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh
