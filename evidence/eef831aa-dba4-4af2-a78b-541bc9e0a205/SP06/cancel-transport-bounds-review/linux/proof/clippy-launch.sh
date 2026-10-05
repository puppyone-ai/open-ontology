#!/bin/bash
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a205/SP06/cancel-transport-bounds-review
PROOF=${EVIDENCE}/linux/proof
ROOT=/tmp/oo-a205-eef831aa-dba4-4af2-a78b-541bc9e0a205
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a205-clippy
NET=oo-a205-net
CARGO_VOL=oo-a205-cargo
TARGET_VOL=oo-a205-target
chmod 755 "$PROOF/clippy-inside.sh" "$PROOF/clippy-launch.sh"
if [ -f "$PROOF/clippy-console.log" ]; then
  echo "refusing to overwrite clippy-console.log" >&2
  exit 2
fi
ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$PROOF/clippy-host-cargo.txt" || true
host_n=$(grep -c . "$PROOF/clippy-host-cargo.txt" || true)
docker ps --filter label=oo.kind=rust-builder --format '{{.Names}}' > "$PROOF/clippy-builders.txt" || true
builder_n=$(grep -c . "$PROOF/clippy-builders.txt" || true)
global_n=$((host_n + builder_n))
printf 'host_cargo=%s rust_builders=%s global=%s cap=6\n' "$host_n" "$builder_n" "$global_n" > "$PROOF/clippy-capacity.txt"
if [ "$global_n" -ge 6 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  exit 3
fi
own=$(docker ps -q --filter label=oo.owner=a205 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  exit 3
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
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e RUSTUP_HOME=/cargo-home/rustup \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/overlay/query_executor_pg_live.rs,target=/src/crates/oo-gateway/tests/query_executor_pg_live.rs,readonly" \
  --mount "type=bind,source=${PROOF}/clippy-inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a205 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a205 \
  --label oo.task=cancel-transport-bounds-review-clippy \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$PROOF/clippy-console.log" 2>&1 || run_exit=$?
run_exit=${run_exit:-0}
printf '%s\n' "$run_exit" > "$PROOF/clippy-docker.exit"
docker inspect -f 'ExitCode={{.State.ExitCode}} OOMKilled={{.State.OOMKilled}} Privileged={{.HostConfig.Privileged}} CapDrop={{json .HostConfig.CapDrop}} Memory={{.HostConfig.Memory}}' "$NAME" > "$PROOF/clippy-hostconfig.txt" || true
docker rm "$NAME" > "$PROOF/clippy-removed.txt"
exit "$run_exit"
