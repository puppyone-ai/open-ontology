#!/bin/bash
# One proof builder. Mounts the fixed owned sources over the immutable archive.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a196/SP06/cancel-transport-bounds
PROOF=${EVIDENCE}/linux/proof
ROOT=/tmp/oo-a196-eef831aa-dba4-4af2-a78b-541bc9e0a196
SEC=/tmp/oo-a196-eef831aa-dba4-4af2-a78b-541bc9e0a196/secrets/client
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a196-proof
NET=oo-a196-net
CARGO_VOL=oo-a196-cargo
TARGET_VOL=oo-a196-target

chmod 755 "$PROOF/inside.sh" "$PROOF/launch.sh"
if [ -f "$PROOF/console.log" ]; then
  echo "refusing to overwrite proof console.log" >&2
  exit 2
fi
if [ ! -f "$ROOT/archive/crates/oo-gateway/src/query_executor.rs" ]; then
  echo "immutable archive is missing" >&2
  exit 2
fi
if [ ! -d "$SEC" ]; then
  echo "postgres client secrets are missing" >&2
  exit 2
fi
ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$PROOF/host-cargo-procs.txt" || true
host_n=$(grep -c . "$PROOF/host-cargo-procs.txt" || true)
docker ps --filter label=oo.kind=rust-builder --format '{{.Names}}' > "$PROOF/builders-before.txt" || true
builder_n=$(grep -c . "$PROOF/builders-before.txt" || true)
global_n=$((host_n + builder_n))
printf 'host_cargo=%s rust_builders=%s global=%s cap=6\n' "$host_n" "$builder_n" "$global_n" > "$PROOF/capacity.txt"
if [ "$global_n" -ge 6 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$PROOF/docker-run.exit"
  exit 3
fi
own=$(docker ps -q --filter label=oo.owner=a196 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$PROOF/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named proof container already exists" >&2
  exit 2
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$PROOF/image.txt"
grep -q 'arch=arm64' "$PROOF/image.txt"
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL image=pinned-rust-1.94 bash /inside.sh" > "$PROOF/invocation.txt"
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
  -e OO_A189_LIVE=1 \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${SRC}/crates/oo-gateway/src/query_executor.rs,target=/src/crates/oo-gateway/src/query_executor.rs,readonly" \
  --mount "type=bind,source=${SRC}/crates/oo-gateway/tests/query_executor_pg_live.rs,target=/src/crates/oo-gateway/tests/query_executor_pg_live.rs,readonly" \
  --mount "type=bind,source=${PROOF}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${SEC},target=/run/oo-a189/fixture,readonly" \
  --tmpfs /tmp:rw,nosuid,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a196 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a196 \
  --label oo.task=cancel-transport-bounds-proof \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$PROOF/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$PROOF/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME" 2>/dev/null || echo missing)"
  echo "PortBindings=$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$NAME" 2>/dev/null || echo missing)"
} > "$PROOF/hostconfig.txt"
docker inspect -f '{{json .Mounts}}' "$NAME" > "$PROOF/mounts.txt" 2>/dev/null || true
docker rm "$NAME" > "$PROOF/container-removed.txt" 2>/dev/null || true
printf '%s\n' "$run_exit" > "$PROOF/proof.exit"
exit "$run_exit"
