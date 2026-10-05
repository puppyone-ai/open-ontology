#!/bin/bash
# One Linux arm64 Rust 1.94 builder. Immutable archive of HEAD. No PostgreSQL.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a187/SP06/parser-worker
SMOKE=${EVIDENCE}/linux/smoke
ROOT=/tmp/oo-a187-eef831aa-dba4-4af2-a78b-541bc9e0a187
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a187-smoke
NET=oo-a187-net
CARGO_VOL=oo-a187-cargo
TARGET_VOL=oo-a187-target

chmod 755 "$SMOKE/inside.sh" "$SMOKE/launch.sh" "$SMOKE/libclang-extract.sh"
mkdir -p "$SMOKE/phases"
if [ -f "$SMOKE/console.log" ]; then
  echo "refusing to overwrite smoke console.log" >&2
  printf '%s\n' 2 > "$SMOKE/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$SMOKE/docker-run.exit"
  exit 1
fi

git -C "$SRC" rev-parse HEAD > "$SMOKE/archive-head.txt"
git -C "$SRC" merge-base --is-ancestor 8a289f0c699abb8c707ca45ad45cb036abc3fc05 HEAD
echo "ancestor_8a289f0c=yes" >> "$SMOKE/archive-head.txt"
mkdir -p "$ROOT"
if [ ! -f "$ROOT/archive/crates/oo-gateway/src/sql_guard.rs" ]; then
  rm -rf "$ROOT/archive"
  mkdir -p "$ROOT/archive"
  git -C "$SRC" archive HEAD | tar -x -C "$ROOT/archive"
fi
if [ -f "$ROOT/archive/crates/oo-gateway/src/parser_worker.rs" ]; then
  echo "archive already contains parser_worker.rs" >&2
  printf '%s\n' 2 > "$SMOKE/docker-run.exit"
  exit 2
fi
git -C "$SRC" rev-parse HEAD > "$SMOKE/archive-commit.txt"
sha256sum "$ROOT/archive/Cargo.lock" | awk '{print $1}' > "$SMOKE/archive-lock.sha256"
sha256sum "$ROOT/archive/crates/oo-control/src/file_hosting.rs" | awk '{print $1}' > "$SMOKE/archive-control.sha256"
sha256sum "$SRC/crates/oo-control/src/file_hosting.rs" | awk '{print $1}' > "$SMOKE/worktree-control.sha256"
printf '%s\n' "parser_worker.rs absent in archive" > "$SMOKE/archive-product.txt"

docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a187 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a187 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} {{.Driver}} internal={{.Internal}}' "$NET" > "$SMOKE/network.txt"

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$SMOKE/host-cargo-procs.txt" || true
host_n=$(grep -c . "$SMOKE/host-cargo-procs.txt" || true)
{
  docker ps --filter label=oo.kind=rust-builder --format '{{.Names}}'
  docker ps --filter ancestor=sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 --format '{{.Names}}'
} | awk 'NF && !seen[$0]++' > "$SMOKE/containers-before.txt" || true
builder_n=$(grep -c . "$SMOKE/containers-before.txt" || true)
global_n=$((host_n + builder_n))
printf 'host_cargo=%s rust_builders=%s global=%s cap=3\n' "$host_n" "$builder_n" "$global_n" > "$SMOKE/capacity.txt"
if [ "$global_n" -ge 3 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$SMOKE/docker-run.exit"
  exit 3
fi
own=$(docker ps -aq --filter label=oo.owner=a187 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$SMOKE/docker-run.exit"
  exit 3
fi

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$SMOKE/image.txt"
grep -q 'arch=arm64' "$SMOKE/image.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a187 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a187 \
  --label oo.kind=cargo \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a187 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a187 \
  --label oo.kind=target \
  "$TARGET_VOL" >/dev/null
printf '%s\n' "$CARGO_VOL" > "$SMOKE/volume-cargo.txt"
printf '%s\n' "$TARGET_VOL" > "$SMOKE/volume-target.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named smoke container already exists" >&2
  printf '%s\n' 2 > "$SMOKE/docker-run.exit"
  exit 2
fi

cat > "$SMOKE/invocation.txt" <<EOF
run_id=eef831aa-dba4-4af2-a78b-541bc9e0a187
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
msrv_override=RUSTUP_TOOLCHAIN=1.94.0
network=${NET}
container=${NAME}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
privileged=no
network_mode=bridge_not_host
home_mount=no
socket_mount=no
host_port=none
shared_cargo_cache=no
pull=never
postgres=none
phase=preedit
global_cap=3
archive=${ROOT}/archive
EOF

set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --memory 8g \
  --memory-swap 8g \
  --cpus 4 \
  --pids-limit 4096 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${SMOKE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${SMOKE}/libclang-extract.sh,target=/libclang-extract.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a187 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a187 \
  --label oo.task=parser-worker-smoke \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$SMOKE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$SMOKE/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
  echo "CapAdd=$(docker inspect -f '{{json .HostConfig.CapAdd}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "PortBindings=$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$NAME")"
  echo "Tmpfs=$(docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$SMOKE/hostconfig.txt"
docker inspect -f '{{json .Mounts}}' "$NAME" > "$SMOKE/mounts.txt"
docker rm "$NAME" > "$SMOKE/container-removed.txt"
exit "$run_exit"
