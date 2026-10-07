#!/bin/bash
# Pre-edit smoke launcher for a326. Refuses to overwrite console.log. Does not prune.
# Shape from the reviewed a319 preoverlay launcher, which records a204 as its origin.
set -u
SRC=/Users/redux/puppy-one/open-ontology
BASE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http
HARNESS=${BASE}/linux/smoke-2
EVIDENCE=${BASE}/linux/smoke-2
ROOT=/tmp/oo-a326-eef831aa-dba4-4af2-a78b-541bc9e0a326/smoke-2
FIXED=e8d297f95b32052ea587fe8ad67b6e8493a00300
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a326-smoke-2
NET=oo-a326-eef831aa-net
CARGO_VOL=oo-a326-eef831aa-cargo
TARGET_VOL=oo-a326-eef831aa-target

chmod 755 "$HARNESS/inside.sh" "$HARNESS/launch.sh" "$HARNESS/extract-libclang.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite smoke-2 console.log" >&2
  exit 2
fi
mkdir -p "$EVIDENCE"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive" "$BASE/source/blobs"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml crates \
  | tar -x -C "$ROOT/archive"
if [ ! -f "$ROOT/archive/crates/oo-gateway/src/lib.rs" ]; then
  echo "archive is missing oo-gateway" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ -f "$ROOT/archive/crates/oo-gateway/src/http_gateway.rs" ] \
  || [ -f "$ROOT/archive/crates/oo-gateway/build.rs" ]
then
  echo "archive already contains the product overlay" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
blob_ok=0
{
  while IFS='|' read -r path name; do
    [ -n "$path" ] || continue
    want=$(git -C "$SRC" rev-parse "$FIXED:$path")
    got=$(git -C "$SRC" hash-object "$ROOT/archive/$path")
    printf '%s %s want=%s got=%s\n' "$path" "$name" "$want" "$got"
    if [ "$want" != "$got" ]; then
      echo "blob mismatch $path" >&2
      blob_ok=1
    fi
    cp "$ROOT/archive/$path" "$BASE/source/blobs/$name"
  done <<'BLOBS'
Cargo.toml|Cargo.toml
Cargo.lock|Cargo.lock
rust-toolchain.toml|rust-toolchain.toml
crates/oo-gateway/Cargo.toml|oo-gateway-Cargo.toml
crates/oo-gateway/src/lib.rs|oo-gateway-lib.rs
crates/oo-gateway/src/bin/oo-gateway.rs|oo-gateway-bin.rs
BLOBS
} > "$EVIDENCE/archive-blobs.txt"
if [ "$blob_ok" -ne 0 ]; then
  echo "archive blob mismatch" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
printf '%s\n' "overlay=absent archive=$FIXED" > "$EVIDENCE/overlay-absent.txt"
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/fixed-tree.txt"
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt" || { echo "pinned image is not arm64" >&2; printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"; exit 2; }
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a326 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a326 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a326 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a326 \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a326 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a326 \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" >/dev/null
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
  echo "named smoke container already exists" >&2
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
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${HARNESS}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${HARNESS}/extract-libclang.sh,target=/extract-libclang.sh,readonly" \
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
