#!/bin/bash
# One Linux arm64 Rust 1.94 builder. Baseline archive plus the repro test overlay.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a189/SP06/query-executor-shutdown
PROBE=${EVIDENCE}/linux/probe
ROOT=/tmp/oo-a189-eef831aa-dba4-4af2-a78b-541bc9e0a189
SEC=/tmp/oo-a189-eef831aa-dba4-4af2-a78b-541bc9e0a189/secrets/client
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
FIXED=8a289f0c699abb8c707ca45ad45cb036abc3fc05
NAME=oo-a189-repro
NET=oo-a189-net
CARGO_VOL=oo-a189-cargo
TARGET_VOL=oo-a189-target
OVERLAY="$ROOT/archive/crates/oo-gateway/tests/query_executor_repro.rs"

chmod 755 "$PROBE/inside.sh" "$PROBE/launch.sh"
if [ -f "$PROBE/console.log" ]; then
  echo "refusing to overwrite probe console.log" >&2
  exit 2
fi
if [ ! -f "$SEC/ca.crt" ] || [ ! -f "$SEC/admin.pass" ] || [ ! -f "$SEC/login.pass" ]; then
  echo "client fixture material is missing" >&2
  exit 2
fi
if [ -f "$ROOT/archive/crates/oo-gateway/src/parser_worker.rs" ]; then
  echo "immutable archive contains parser_worker.rs" >&2
  exit 2
fi
git -C "$SRC" show "${FIXED}:crates/oo-gateway/src/query_executor.rs" > "$ROOT/archive/crates/oo-gateway/src/query_executor.rs"
cp "$PROBE/repro.rs" "$OVERLAY"
cleanup_overlay() {
  rm -f "$OVERLAY"
}
trap cleanup_overlay EXIT
(
  cd "$ROOT/archive"
  shasum -a 256 crates/oo-gateway/src/query_executor.rs crates/oo-gateway/tests/query_executor_repro.rs
) > "$PROBE/overlay-sha256.txt"
cmp -s "$ROOT/archive/crates/oo-gateway/src/lib.rs" <(git -C "$SRC" show "${FIXED}:crates/oo-gateway/src/lib.rs")
ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$PROBE/host-cargo-procs.txt" || true
host_n=$(grep -c . "$PROBE/host-cargo-procs.txt" || true)
docker ps --filter label=oo.kind=rust-builder --format '{{.Names}}' > "$PROBE/builders-before.txt" || true
builder_n=$(grep -c . "$PROBE/builders-before.txt" || true)
global_n=$((host_n + builder_n))
printf 'host_cargo=%s rust_builders=%s global=%s cap=3\n' "$host_n" "$builder_n" "$global_n" > "$PROBE/capacity.txt"
if [ "$global_n" -ge 3 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$PROBE/docker-run.exit"
  exit 3
fi
own=$(docker ps -q --filter label=oo.owner=a189 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$PROBE/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named repro container already exists" >&2
  printf '%s\n' 2 > "$PROBE/docker-run.exit"
  exit 2
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$PROBE/image.txt"
grep -q 'arch=arm64' "$PROBE/image.txt"
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL image=pinned-rust-1.94 bash /inside.sh" > "$PROBE/invocation.txt"
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
  -e OO_A189_REPRO=1 \
  -e OO_A189_ORACLE=/evidence/linux/probe/oracle.txt \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${PROBE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${SEC},target=/run/oo-a189/fixture,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a189 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a189 \
  --label oo.task=query-executor-shutdown-repro \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$PROBE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$PROBE/docker-run.exit"
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
} > "$PROBE/hostconfig.txt"
docker inspect -f '{{json .Mounts}}' "$NAME" > "$PROBE/mounts.txt" 2>/dev/null || true
docker rm "$NAME" > "$PROBE/container-removed.txt" 2>/dev/null || true
printf '%s\n' "$run_exit" > "$PROBE/repro.exit"
exit "$run_exit"
