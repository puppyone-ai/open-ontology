#!/bin/bash
# One Linux 1.94 builder for the import command. Working tree is mounted read-only.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a162/SP04/import-pg-cli/linux/build
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a162-build
NET=oo-a162-eef831aa-net
CARGO_VOL=oo-a162-eef831aa-cargo
TARGET_VOL=oo-a162-eef831aa-target

mkdir -p "$EVIDENCE"
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite build console.log" >&2
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  exit 1
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  echo "a162 network is absent" >&2
  exit 2
fi
own=$(docker ps -q --filter label=oo.owner=a162 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own=%s running_rust=%s host_cargo=%s global=%s\n' "$own" "$running_rust" "$host_cargo" "$global_builders" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$((running_rust + host_cargo))" -ge 2 ] || [ "$global_builders" -ge 2 ]; then
  echo "deferred: rust builder capacity" >&2
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named build container already exists" >&2
  exit 2
fi
docker run --name "$NAME" \
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
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  --mount "type=bind,source=${SRC},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a162 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a162 \
  --label oo.task=import-pg-cli-build \
  --label oo.kind=rust-builder \
  --label oo.platform=linux/arm64 \
  "$IMAGE" \
  bash /evidence/inside.sh > "$EVIDENCE/console.log" 2>&1
printf '%s\n' $? > "$EVIDENCE/docker-run.exit"
docker inspect -f 'status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}}' "$NAME" > "$EVIDENCE/hostconfig.txt"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
