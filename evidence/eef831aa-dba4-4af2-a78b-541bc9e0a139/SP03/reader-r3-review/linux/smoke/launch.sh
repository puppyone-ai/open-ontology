#!/bin/bash
# One owned linux/arm64 Rust 1.94 smoke builder. Exact archive of 58afa97. No overlay.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a139/SP03/reader-r3-review/linux/smoke
ROOT=/tmp/oo-a139-eef831aa
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a139-msrv-smoke
NET=oo-a139-eef831aa-net
CARGO_VOL=oo-a139-eef831aa-cargo
TARGET_VOL=oo-a139-eef831aa-target
FIXED=58afa97ded85a7dd89c72532bfc8973c45255d7c
TREE=543fd103532a8366ce92a0b4eaea4e089eb76d2d
SNAP=${ROOT}/smoke-src

mkdir -p "$EVIDENCE" "$ROOT"
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"

docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}' > "$EVIDENCE/containers-before.txt"
ps -axo pid=,ppid=,etime=,comm= > "$EVIDENCE/processes-comm-only.txt"
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a139 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ]; then
  echo "deferred: this task already has a running rust builder" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if [ "$((running_rust + host_cargo))" -ge 2 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi

if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a139 \
    --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a139 \
    "$NET" > "$EVIDENCE/network.txt"
fi

actual_tree=$(git -C "$SRC" rev-parse "${FIXED}^{tree}")
test "$actual_tree" = "$TREE"
rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive "$FIXED" | tar -x -C "$SNAP"
printf '%s\n' "$FIXED" > "$EVIDENCE/archive-base.txt"
printf '%s\n' "$actual_tree" > "$EVIDENCE/archive-tree.txt"
test -f "$SNAP/crates/oo-connectors/src/key_plan.rs"
test ! -f "$SNAP/crates/oo-connectors/tests/pg_reader_r3_review.rs"
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/workspace-head.txt"
test "$(cat "$EVIDENCE/workspace-head.txt")" != "$FIXED" || true

record_blob() {
  rel=$1
  name=$2
  committed=$(git -C "$SRC" rev-parse "${FIXED}:${rel}")
  archived=$(git -C "$SRC" hash-object "$SNAP/$rel")
  printf 'commit=%s blob=%s\n' "$FIXED" "$committed" > "$EVIDENCE/${name}-blob.txt"
  test "$committed" = "$archived"
  shasum -a 256 "$SNAP/$rel" | awk '{print $1}' > "$EVIDENCE/expect-${name}.sha256"
}
record_blob crates/oo-connectors/src/key_plan.rs key-plan
record_blob crates/oo-connectors/src/snapshot.rs snapshot
record_blob crates/oo-connectors/src/sql.rs sql
record_blob crates/oo-connectors/src/stream.rs stream
record_blob crates/oo-connectors/src/error.rs error
record_blob crates/oo-connectors/src/session.rs session
record_blob crates/oo-connectors/src/config.rs config
record_blob crates/oo-connectors/src/lib.rs lib
record_blob crates/oo-connectors/src/types/mod.rs types-mod
record_blob crates/oo-connectors/src/types/read.rs types-read

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology --label oo.owner=a139 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a139 --label oo.kind=cargo \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology --label oo.owner=a139 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a139 --label oo.kind=target \
  "$TARGET_VOL" >/dev/null

if docker container inspect "$NAME" >/dev/null 2>&1; then
  prior=$(docker inspect -f '{{.State.Status}}' "$NAME")
  printf 'prior_status=%s\n' "$prior" > "$EVIDENCE/prior-container.txt"
  if [ "$prior" = "running" ]; then
    echo "deferred: named container is already running" >&2
    printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
    exit 3
  fi
  docker rm "$NAME" >> "$EVIDENCE/prior-container.txt"
fi

cat > "$EVIDENCE/invocation.txt" <<EOF
source_commit=${FIXED}
tree=${actual_tree}
keys_commit=8bda0cd48976b23d268d125b34bc8130d94b8253
composition=exact 58afa97 archive, no review overlay
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
container=${NAME}
network=${NET}
pull=never
privileged=no
home_mount=no
socket_mount=no
host_network=no
shared_cargo_cache=no
EOF

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
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/tmp \
  --mount "type=bind,source=${SNAP},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a139 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a139 \
  --label oo.task=reader-r3-review-smoke \
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
  echo "Status=$(docker inspect -f '{{.State.Status}}' "$NAME")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{.HostConfig.CapDrop}}' "$NAME")"
} > "$EVIDENCE/hostconfig.txt"
exit "$run_exit"
