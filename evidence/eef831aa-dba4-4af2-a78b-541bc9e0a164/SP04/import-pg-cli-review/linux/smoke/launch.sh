#!/bin/bash
# One Linux 1.94 builder. Immutable archive only. No PostgreSQL. No overlay.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a164/SP04/import-pg-cli-review/linux/smoke
ROOT=/tmp/oo-a164-eef831aa-dba4-4af2-a78b-541bc9e0a164
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a164-smoke
NET=oo-a164-eef831aa-net
CARGO_VOL=oo-a164-eef831aa-cargo
TARGET_VOL=oo-a164-eef831aa-target

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite smoke console.log" >&2
  exit 2
fi
if [ ! -d "$ROOT/archive/crates/oo-pipeline" ]; then
  echo "immutable archive is absent" >&2
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  exit 1
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a164 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a164 \
  --label oo.kind=network \
  "$NET" >/dev/null
own=$(docker ps -q --filter label=oo.owner=a164 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own=%s running_rust=%s host_cargo=%s global=%s\n' "$own" "$running_rust" "$host_cargo" "$global_builders" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$((running_rust + host_cargo))" -ge 2 ] || [ "$global_builders" -ge 2 ]; then
  echo "deferred: rust builder capacity" >&2
  exit 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a164 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a164 \
  --label oo.kind=cargo \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a164 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a164 \
  --label oo.kind=target \
  "$TARGET_VOL" >/dev/null
cp "$EVIDENCE/expect-sha256.txt" "$EVIDENCE/expect-sha256.txt"
printf '%s\n' "docker run --name $NAME --platform linux/arm64 --pull never --network $NET --cap-drop ALL image=pinned-rust-1.94 bash /evidence/inside.sh" > "$EVIDENCE/invocation.txt"
set +e
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
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a164 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a164 \
  --label oo.task=import-pg-cli-review-smoke \
  --label oo.kind=rust-builder \
  --label oo.platform=linux/arm64 \
  "$IMAGE" \
  bash /evidence/inside.sh > "$EVIDENCE/console.log" 2>&1
run_code=$?
set -e
printf '%s\n' "$run_code" > "$EVIDENCE/docker-run.exit"
docker inspect -f 'status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} tmpfs={{json .HostConfig.Tmpfs}}' "$NAME" > "$EVIDENCE/hostconfig.txt"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
exit "$run_code"
