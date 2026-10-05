#!/bin/bash
# One Linux arm64 Rust 1.94 builder. Immutable archive of HEAD before edits.
# Owned executor bytes must match baseline 15d23fe9. No PostgreSQL.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a196/SP06/cancel-transport-bounds
SMOKE=${EVIDENCE}/linux/smoke
ROOT=/tmp/oo-a196-eef831aa-dba4-4af2-a78b-541bc9e0a196
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
BASELINE=15d23fe9f2cbc74e3f3278d70d0f56390568126e
NAME=oo-a196-smoke
NET=oo-a196-net
CARGO_VOL=oo-a196-cargo
TARGET_VOL=oo-a196-target

chmod 755 "$SMOKE/inside.sh" "$SMOKE/launch.sh" "$SMOKE/extract-libclang.sh"
if [ -f "$SMOKE/console.log" ]; then
  echo "refusing to overwrite smoke console.log" >&2
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  exit 1
fi
HEAD=$(git -C "$SRC" rev-parse HEAD)
printf '%s\n' "$HEAD" > "$SMOKE/fixed-commit.txt"
printf '%s\n' "$BASELINE" > "$SMOKE/baseline-commit.txt"
mkdir -p "$ROOT/archive"
if [ ! -f "$ROOT/archive/crates/oo-gateway/src/query_executor.rs" ]; then
  git -C "$SRC" archive --format=tar "$HEAD" | tar -xf - -C "$ROOT/archive"
fi
git -C "$SRC" rev-parse "${HEAD}^{tree}" > "$SMOKE/archive-tree.txt"
for rel in \
  crates/oo-gateway/src/query_executor.rs \
  crates/oo-gateway/tests/query_executor_pg_live.rs \
  contracts/gateway/query-executor-v1.md
do
  git -C "$SRC" diff --exit-code "$BASELINE" HEAD -- "$rel"
  cmp -s "$ROOT/archive/${rel}" <(git -C "$SRC" show "${BASELINE}:${rel}")
done
printf '%s\n' "head=${HEAD} baseline=${BASELINE} owned=match" > "$SMOKE/owned-overlay.txt"
(
  cd "$ROOT/archive"
  shasum -a 256 \
    crates/oo-gateway/src/query_executor.rs \
    crates/oo-gateway/tests/query_executor_pg_live.rs \
    contracts/gateway/query-executor-v1.md
) > "$SMOKE/archive-hashes.txt"

docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a196 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a196 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} {{.Driver}} internal={{.Internal}}' "$NET" > "$SMOKE/network.txt"
ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$SMOKE/host-cargo-procs.txt" || true
host_n=$(grep -c . "$SMOKE/host-cargo-procs.txt" || true)
docker ps --filter label=oo.kind=rust-builder --format '{{.Names}}' > "$SMOKE/builders-before.txt" || true
builder_n=$(grep -c . "$SMOKE/builders-before.txt" || true)
global_n=$((host_n + builder_n))
printf 'host_cargo=%s rust_builders=%s global=%s cap=6\n' "$host_n" "$builder_n" "$global_n" > "$SMOKE/capacity.txt"
if [ "$global_n" -ge 6 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$SMOKE/docker-run.exit"
  exit 3
fi
own=$(docker ps -q --filter label=oo.owner=a196 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$SMOKE/docker-run.exit"
  exit 3
fi
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a196 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a196 \
  --label oo.kind=cargo \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a196 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a196 \
  --label oo.kind=target \
  "$TARGET_VOL" >/dev/null
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named smoke container already exists" >&2
  printf '%s\n' 2 > "$SMOKE/docker-run.exit"
  exit 2
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$SMOKE/image.txt"
grep -q 'arch=arm64' "$SMOKE/image.txt"
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL image=pinned-rust-1.94 bash /inside.sh" > "$SMOKE/invocation.txt"
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
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${SMOKE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${SMOKE}/extract-libclang.sh,target=/extract-libclang.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a196 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a196 \
  --label oo.task=cancel-transport-bounds-smoke \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$SMOKE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$SMOKE/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapAdd=$(docker inspect -f '{{json .HostConfig.CapAdd}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME" 2>/dev/null || echo missing)"
  echo "PortBindings=$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME" 2>/dev/null || echo missing)"
} > "$SMOKE/hostconfig.txt"
docker inspect -f '{{json .Mounts}}' "$NAME" > "$SMOKE/mounts.txt" 2>/dev/null || true
docker rm "$NAME" > "$SMOKE/container-removed.txt" 2>/dev/null || true
printf '%s\n' "$run_exit" > "$SMOKE/smoke.exit"
exit "$run_exit"
