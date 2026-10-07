#!/bin/bash
# One 6GiB arm64 builder. Shape from linux/compile/launch.sh.
# Archive is HEAD plus the owned oo-gateway overlay and the copied consumer lock.
# It does not overlay a327 files and it does not prune.
set -u
SRC=/Users/redux/puppy-one/open-ontology
BASE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http
HARNESS=${BASE}/linux/compile-4
EVIDENCE=${BASE}/linux/compile-4
ROOT=/tmp/oo-a326-eef831aa-dba4-4af2-a78b-541bc9e0a326/compile-4
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a326-compile-4
NET=oo-a326-eef831aa-net
CARGO_VOL=oo-a326-eef831aa-cargo
TARGET_VOL=oo-a326-eef831aa-target

chmod 755 "$HARNESS/inside.sh" "$HARNESS/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite compile-4 console.log" >&2
  exit 2
fi
mkdir -p "$EVIDENCE"
if [ ! -f "$EVIDENCE/Cargo.lock.pre-consumer" ]; then
  echo "pre-consumer lock is missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive"
git -C "$SRC" archive --format=tar HEAD \
  Cargo.toml Cargo.lock rust-toolchain.toml crates tools/gateway-token-store \
  | tar -x -C "$ROOT/archive"
overlay() {
  cp "$SRC/$1" "$ROOT/archive/$1"
}
overlay Cargo.lock
overlay crates/oo-gateway/Cargo.toml
overlay crates/oo-gateway/build.rs
overlay crates/oo-gateway/src/lib.rs
overlay crates/oo-gateway/src/http_gateway.rs
overlay crates/oo-gateway/src/pack_catalog.rs
overlay crates/oo-gateway/src/bin/oo-gateway.rs
mkdir -p "$ROOT/archive/crates/oo-gateway/tests"
overlay crates/oo-gateway/tests/http_gateway_loopback.rs
overlay crates/oo-gateway/tests/http_gateway_live.rs
overlay crates/oo-gateway/tests/query_executor_pg_live.rs
overlay crates/oo-gateway/src/policy_catalog.rs
overlay crates/oo-gateway/src/policy.rs
overlay crates/oo-gateway/tests/column_acl_set_pg_live.rs
if [ ! -f "$ROOT/archive/crates/oo-gateway/src/http_gateway.rs" ]; then
  echo "overlay missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if cmp -s "$ROOT/archive/Cargo.lock" "$EVIDENCE/Cargo.lock.pre-consumer"; then
  echo "archive lock is still the pre-consumer lock" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt" || { echo "pinned image is not arm64" >&2; printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"; exit 2; }
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a326 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a326 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
own=$(docker ps -q --filter label=oo.owner=a326 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named compile container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
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
  --mount "type=bind,source=${ROOT}/archive,target=/src" \
  --mount "type=bind,source=${HARNESS}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${BASE}/linux/smoke-2/extract-libclang.sh,target=/extract-libclang.sh,readonly" \
  --mount "type=bind,source=${BASE}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a326 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a326 \
  --label oo.task=gateway-http \
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
  echo "ReadonlyRootfs=$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$EVIDENCE/hostconfig.txt"
docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME" > "$EVIDENCE/tmpfs.txt"
docker inspect -f '{{range .Mounts}}{{.Type}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$EVIDENCE/mounts.txt"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
exit "$run_exit"
