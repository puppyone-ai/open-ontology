#!/bin/bash
# One linux/arm64 Rust 1.94 builder for session-control-api. Reuses the precode network and cargo volume.
# Does not prune. Does not touch foreign containers. Host cargo is refused.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a387/SP11/session-control-api/linux/impl
FIX=/tmp/oo-a387-eef831aa-dba4-4af2-a78b-541bc9e0a387/fixture
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a387-api
NET=oo-a387-eef831aa-net
CARGO_VOL=oo-a387-eef831aa-cargo
TARGET_VOL=oo-a387-eef831aa-target-api
PARENT=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a295/SP11/session-release/linux/precode

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite impl console.log" >&2
  exit 2
fi
rm -f "$EVIDENCE"/*.exit "$EVIDENCE"/*.stdout "$EVIDENCE"/*.stderr "$EVIDENCE"/hostconfig.txt "$EVIDENCE"/tmpfs.txt "$EVIDENCE"/mounts.txt "$EVIDENCE"/container-removed.txt
if [ ! -f "$FIX/endpoint.txt" ] || [ ! -f "$FIX/ca.crt" ]; then
  echo "fixture is not ready" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
{
  echo "Rust impl recipe origin"
  echo "direct_parent_path=${PARENT}/launch.sh"
  echo "direct_parent_sha256=$(shasum -a 256 "$PARENT/launch.sh" | awk '{print $1}')"
  echo "direct_parent_inside=${PARENT}/inside.sh"
  echo "direct_parent_inside_sha256=$(shasum -a 256 "$PARENT/inside.sh" | awk '{print $1}')"
  echo "minimal_changes=uuid a387; mount session-control-api rw; siblings and root lock ro; private PG alias; one generate-lockfile for this workspace only"
  echo "image=${IMAGE}"
} > "$EVIDENCE/recipe-origin.txt"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker network inspect "$NET" >/dev/null 2>&1 || {
  echo "task network is missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
}
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || {
  echo "precode cargo volume is missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
}
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a387 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a387 \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" >/dev/null
own=$(docker ps -q --filter label=oo.owner=a387 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named impl container already exists" >&2
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
  -e OO_A387_FIXTURE=/fixture \
  -e OO_ROOT_LOCK=/root.lock \
  --mount "type=bind,source=${SRC}/tools/session-control-api,target=/src/tools/session-control-api" \
  --mount "type=bind,source=${SRC}/tools/session-store,target=/src/tools/session-store,readonly" \
  --mount "type=bind,source=${SRC}/tools/session-transition,target=/src/tools/session-transition,readonly" \
  --mount "type=bind,source=${SRC}/tools/session-release,target=/src/tools/session-release,readonly" \
  --mount "type=bind,source=${SRC}/tools/gateway-token-store,target=/src/tools/gateway-token-store,readonly" \
  --mount "type=bind,source=${SRC}/Cargo.lock,target=/root.lock,readonly" \
  --mount "type=bind,source=${FIX},target=/fixture,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=512m \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a387 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a387 \
  --label oo.task=session-control-api-impl \
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
