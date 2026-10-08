#!/bin/bash
# One linux/arm64 builder. Runs Clippy only. Does not repeat the green test campaign.
# Does not prune. Does not touch Postgres or foreign containers.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a394/SP11/session-control-api-review/linux/proof/clippy-final
ARCHIVE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a394/SP11/session-control-api-review/archive/tree
ORACLE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a394/SP11/session-control-api-review/oracle/a394_oracle.rs
FIX=/tmp/oo-a394-eef831aa-dba4-4af2-a78b-541bc9e0a394/fixture
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a394-review
NET=oo-a394-eef831aa-net
CARGO_VOL=oo-a394-eef831aa-cargo
TARGET_VOL=oo-a394-eef831aa-target
INSIDE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a394/SP11/session-control-api-review/linux/proof/inside-clippy.sh

mkdir -p "$EVIDENCE"
chmod 755 "$INSIDE" "$0"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite clippy-final/console.log" >&2
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
if [ ! -f "$FIX/endpoint.txt" ] || [ ! -f "$FIX/ca.crt" ]; then
  echo "fixture is not ready" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
docker network inspect "$NET" >/dev/null 2>&1 || {
  echo "task network is missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
}
if ! docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || ! docker volume inspect "$TARGET_VOL" >/dev/null 2>&1; then
  echo "warm cargo volumes are missing; not creating a second cache" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
own=$(docker ps -q --filter label=oo.owner=a394 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named proof container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 bash /inside.sh" > "$EVIDENCE/invocation.txt"
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
  --pids-limit 4096 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  -e OO_ROOT_LOCK=/root.lock \
  --mount "type=bind,source=${ARCHIVE},target=/archive,readonly" \
  --mount "type=bind,source=${ORACLE},target=/oracle/a394_oracle.rs,readonly" \
  --mount "type=bind,source=${FIX},target=/fixture,readonly" \
  --mount "type=bind,source=${INSIDE},target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=512m \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a394 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a394 \
  --label oo.task=session-control-api-review \
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
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME")"
} > "$EVIDENCE/hostconfig.txt"
docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME" > "$EVIDENCE/tmpfs.txt"
docker inspect -f '{{json .Mounts}}' "$NAME" > "$EVIDENCE/mounts.txt"
docker rm "$NAME" >/dev/null
printf '%s\n' removed > "$EVIDENCE/container-removed.txt"
exit "$run_exit"
