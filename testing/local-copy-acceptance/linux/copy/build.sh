#!/bin/bash
# One private Linux arm64 Rust 1.94 builder. No pull. Does not stop other containers.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a409/SP03/local-copy-acceptance/linux/copy/build
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a409-copy-build
NET=oo-a409-eef831aa-net
CARGO_VOL=oo-a409-eef831aa-cargo
TARGET_VOL=oo-a409-eef831aa-target
mkdir -p "$EVIDENCE"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite copy build console" >&2
  exit 2
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology --label oo.owner=a409 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a409 --label oo.kind=network \
  "$NET" >/dev/null
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology --label oo.owner=a409 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a409 --label oo.kind=cargo \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology --label oo.owner=a409 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a409 --label oo.kind=target \
  "$TARGET_VOL" >/dev/null
own=$(docker ps -q --filter label=oo.owner=a409 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
set +e
docker run --name "$NAME" \
  --platform linux/arm64 --pull never --init --user 0:0 \
  --security-opt no-new-privileges --cap-drop ALL \
  --memory 6g --memory-swap 6g --cpus 1 --pids-limit 512 \
  --network "$NET" \
  -e HOME=/tmp -e TMPDIR=/target/tmp -e RUSTUP_TOOLCHAIN=1.94.0 \
  --mount "type=bind,source=${SRC},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${SRC}/testing/local-copy-acceptance/linux/copy/build-inside.sh,target=/inside.sh,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=512m \
  --label oo.project=open-ontology --label oo.owner=a409 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a409 \
  --label oo.task=local-copy-acceptance-build --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 --label oo.kind=rust-builder \
  "$IMAGE" bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
exit "$run_exit"
