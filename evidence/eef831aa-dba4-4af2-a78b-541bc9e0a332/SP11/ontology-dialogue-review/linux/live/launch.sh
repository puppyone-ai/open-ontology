#!/bin/bash
# One Linux arm64 Rust 1.94 builder after preoverlay. Uses the existing private PostgreSQL.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a332/SP11/ontology-dialogue-review
LIVE=${EVIDENCE}/linux/live
PRE=${EVIDENCE}/linux/preoverlay
ROOT=/tmp/oo-a332-eef831aa-dba4-4af2-a78b-541bc9e0a332
FIXED=63566b60aabdf29a29bfe55c4866edf3c4081cfd
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a332-live
NET=oo-a332-eef831aa-net
CARGO_VOL=oo-a332-eef831aa-cargo
TARGET_VOL=oo-a332-eef831aa-target
PGNAME=oo-a332-pg

fail() {
  printf '%s\n' "$1" > "$LIVE/docker-run.exit"
  echo "$2" >&2
  exit "$1"
}

mkdir -p "$LIVE/phases"
printf '%s\n' "$$" > "$LIVE/launch.pid"
chmod 755 "$LIVE/inside.sh" "$LIVE/launch.sh" "$EVIDENCE/linux/libclang-extract.sh"

if [ -f "$LIVE/console.log" ]; then
  echo "refusing to overwrite live console.log" >&2
  exit 2
fi
if [ ! -f "$PRE/docker-run.exit" ] || [ "$(cat "$PRE/docker-run.exit")" != "0" ]; then
  fail 2 "preoverlay did not exit 0"
fi
if [ ! -f "$ROOT/archive-stamp.txt" ] || [ "$(cat "$ROOT/archive-stamp.txt")" != "$FIXED" ]; then
  fail 2 "immutable archive stamp is missing"
fi
if [ ! -d "$ROOT/archive/tools/ontology-dialogue" ]; then
  fail 2 "archive is missing"
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  fail 1 "pinned rust image is not present locally; not pulling"
fi
docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$LIVE/image.txt"
grep -q 'arch=arm64' "$LIVE/image.txt" || fail 1 "rust image is not arm64"
if [ "$(docker ps --filter "name=^/${PGNAME}$" --format '{{.Names}}')" != "$PGNAME" ]; then
  fail 3 "private postgres is not running"
fi
fixture=$(cat "$EVIDENCE/pg/fixture-dir.txt")
if [ ! -s "$fixture/ca.crt" ] || [ ! -s "$fixture/admin.pass" ] || [ ! -s "$fixture/login.pass" ]; then
  fail 2 "private fixture files are missing"
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  fail 2 "named live container already exists"
fi

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$LIVE/host-cargo-procs.txt" || true
host_n=$(grep -c . "$LIVE/host-cargo-procs.txt" || true)
docker ps --filter status=running --filter label=oo.kind=rust-builder --format '{{.Names}}' > "$LIVE/builders-before.txt" || true
builder_n=$(grep -c . "$LIVE/builders-before.txt" || true)
global_n=$((host_n + builder_n))
own=$(docker ps -q --filter status=running --filter label=oo.owner=a332 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
own_pg=$(docker ps -q --filter status=running --filter label=oo.owner=a332 --filter label=oo.kind=postgres | sed '/^$/d' | wc -l | tr -d ' ')
printf 'host_cargo=%s rust_builders=%s global=%s own=%s own_postgres=%s cap_global=10 cap_own=1\n' "$host_n" "$builder_n" "$global_n" "$own" "$own_pg" > "$LIVE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_n" -ge 10 ] || [ "$own_pg" -ne 1 ]; then
  fail 3 "deferred: rust builder capacity or postgres count"
fi

printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 bash /inside.sh" > "$LIVE/invocation.txt"
set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
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
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${LIVE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/linux/libclang-extract.sh,target=/extract-libclang.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/tools,target=/review,readonly" \
  --mount "type=bind,source=${LIVE},target=/evidence/linux/live" \
  --mount "type=bind,source=${fixture},target=/run/oo-a332/fixture,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a332 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a332 \
  --label oo.task=ontology-dialogue-review-live \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$LIVE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$LIVE/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapAdd=$(docker inspect -f '{{json .HostConfig.CapAdd}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME" 2>/dev/null || echo missing)"
  echo "PortBindings=$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME" 2>/dev/null || echo missing)"
} > "$LIVE/hostconfig.txt"
docker rm "$NAME" > "$LIVE/container-removed.txt" 2>&1 || true
exit "$run_exit"
