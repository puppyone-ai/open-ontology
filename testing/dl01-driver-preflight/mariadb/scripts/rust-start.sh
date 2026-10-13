#!/bin/bash
# Starts the one W39 Rust builder. The resource lock is already held by the caller.
set -u
ROOT=/Users/redux/puppy-one/open-ontology
WORK=${ROOT}/testing/dl01-driver-preflight/mariadb
EVIDENCE=${ROOT}/evidence/0d8d559a-7e46-4859-838e-21ffccd62772/DL01/mariadb-driver-preflight
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-dl01w1-39-rust
NET=oo-dl01w1-39
CARGO_VOL=oo-dl01w1-39-cargo
TARGET_VOL=oo-dl01w1-39-target
SESSION=0d8d559a-7e46-4859-838e-21ffccd62772
SECRETS=/tmp/oo-dl01w1-39-secrets
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named rust container already exists" >&2
  exit 2
fi
if ! docker volume inspect "$TARGET_VOL" >/dev/null 2>&1; then
  docker volume create \
    --label oo.project=open-ontology \
    --label oo.owner=DL01-W1-39 \
    --label oo.session="$SESSION" \
    --label oo.kind=target \
    --label oo.prefix=oo-dl01w1-39 \
    "$TARGET_VOL" >/dev/null
fi
mkdir -p "$EVIDENCE/build"
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --memory 6g --cpus 1 sleep infinity" > "$EVIDENCE/build/invocation.txt"
docker run -d --name "$NAME" \
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
  --network "$NET" \
  -e HOME=/tmp \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_BUILD_JOBS=1 \
  -e CARGO_HOME=/cargo \
  -e CARGO_TARGET_DIR=/target \
  -e PATH=/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
  --mount "type=bind,source=${WORK},target=/work" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${SECRETS},target=/run/secrets,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --tmpfs /tmp:rw,nosuid,nodev,size=512m \
  --label oo.project=open-ontology \
  --label oo.owner=DL01-W1-39 \
  --label oo.session="$SESSION" \
  --label oo.task=DL01-W1-39 \
  --label oo.kind=rust-builder \
  --label oo.prefix=oo-dl01w1-39 \
  --label oo.toolchain=1.94.0 \
  "$IMAGE" \
  sleep infinity
