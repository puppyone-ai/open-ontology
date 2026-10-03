#!/bin/bash
# One owned Rust 1.94 builder. Compiles the working tree, not the frozen archive.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a125
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
REPO=/Users/redux/puppy-one/open-ontology
LIB=${REPO}/evidence/${RUN_ID}/SP03/current-schema-guard/lib
NET=${RUN_ID}-net
CARGO_VOL=${RUN_ID}-cargo
TARGET_VOL=${RUN_ID}-target
NAME=${RUN_ID}-rust-lib-${OO_A125_LIB_SUFFIX:-1}

test -f "${REPO}/Cargo.toml"
test -f "${LIB}/inside.sh"
docker image inspect "$IMAGE" >/dev/null
docker network inspect "$NET" >/dev/null

mkdir -p "$LIB"
ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "${LIB}/host-cargo-procs.txt" || true
host_n=$(grep -c . "${LIB}/host-cargo-procs.txt" || true)
docker ps --filter label=oo.kind=rust-builder --format '{{.ID}} {{.Names}} {{.Status}}' > "${LIB}/builders-before.txt" || true
builder_n=$(grep -c . "${LIB}/builders-before.txt" || true)
global_n=$((host_n + builder_n))
{
  echo "captured_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "host_cargo=$host_n"
  echo "rust_builders=$builder_n"
  echo "global=$global_n"
  echo "own_take=1"
  echo "source=working-tree"
} | tee "${LIB}/capacity.txt"
if [ "$global_n" -ge 2 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  exit 3
fi
own=$(docker ps --filter label=oo.owner=a125 --filter label=oo.kind=rust-builder --format '{{.ID}}' || true)
if [ -n "$own" ]; then
  echo "deferred: this session already has a running rust builder" >&2
  exit 3
fi
if docker ps -a --filter "name=^/${NAME}$" --format '{{.ID}}' | grep -q .; then
  state=$(docker inspect -f '{{.State.Status}}' "$NAME")
  echo "refusing to replace existing ${NAME} status=${state}" >&2
  exit 1
fi

chmod 755 "${LIB}/inside.sh" "${LIB}/launch.sh"
echo "lib_start=$(date -u +%Y-%m-%dT%H:%M:%SZ)" | tee "${LIB}/started.txt"
set +e
docker run \
  --name "$NAME" \
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
  --workdir /src \
  --tmpfs /tmp:rw,nosuid,nodev,size=1g \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/tmp \
  --mount "type=bind,source=${REPO},target=/src,readonly" \
  --mount "type=bind,source=${LIB}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a125 \
  --label oo.session="${RUN_ID}" \
  --label oo.task=current-schema-guard \
  --label oo.run_id="${RUN_ID}" \
  --label oo.scope=sp03-current-schema-lib \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "${LIB}/console.log" 2>&1
run_exit=$?
set -e
printf '%s\n' "$run_exit" > "${LIB}/docker-run.exit"
echo "lib_end=$(date -u +%Y-%m-%dT%H:%M:%SZ)" >> "${LIB}/started.txt"
docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}}' > "${LIB}/inspect-safe.txt" || true
mkdir -p "${LIB}/phases"
docker cp "$NAME":/target/phases/. "${LIB}/phases" || true
exit "$run_exit"
