#!/bin/bash
# One owned linux/arm64 Rust 1.94 smoke builder for fixed commit dd5bf441.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a137/SP03/arrow-acl-review/linux/smoke
ROOT=/tmp/oo-a137-eef831aa
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a137-msrv-smoke
NET=oo-a137-eef831aa-net
CARGO_VOL=oo-a137-eef831aa-cargo
TARGET_VOL=oo-a137-eef831aa-target
FIXED=dd5bf44160f949c0ac21a0ce3ddf5ee5e90b4200
ARROW=75b993d0b578fa863a32c64abc0073ed92d51a5d
DRIVER=bd77b2520bcc3d63517bc49309002f25b58544b1
SNAP=${ROOT}/snapshot

mkdir -p "$EVIDENCE" "$ROOT"
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"

docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}' > "$EVIDENCE/containers-before.txt"
ps -axo pid=,ppid=,etime=,comm= > "$EVIDENCE/processes-comm-only.txt"
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a137 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
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
    --label oo.owner=a137 \
    --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a137 \
    "$NET" > "$EVIDENCE/network.txt"
fi

rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive "$FIXED" | tar -x -C "$SNAP"
printf '%s\n' "$FIXED" > "$EVIDENCE/archive-base.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/archive-tree.txt"
test -f "$SNAP/crates/oo-connectors/src/stream.rs"
test -f "$SNAP/crates/oo-connectors/tests/pg_stream_acl_live.rs"
test ! -f "$SNAP/crates/oo-connectors/tests/pg_stream_acl_review.rs"

record_blob() {
  rel=$1
  name=$2
  commit=$3
  committed=$(git -C "$SRC" rev-parse "${commit}:${rel}")
  archived=$(git -C "$SRC" hash-object "$SNAP/$rel")
  printf 'commit=%s blob=%s\n' "$commit" "$committed" > "$EVIDENCE/${name}-blob.txt"
  test "$committed" = "$archived"
  shasum -a 256 "$SNAP/$rel" | awk '{print $1}' > "$EVIDENCE/expect-${name}.sha256"
}
record_blob crates/oo-connectors/src/snapshot.rs snapshot "$FIXED"
record_blob crates/oo-connectors/src/sql.rs sql "$FIXED"
record_blob crates/oo-connectors/src/stream.rs stream "$FIXED"
record_blob crates/oo-connectors/src/error.rs error "$FIXED"
record_blob crates/oo-connectors/src/types/mod.rs types-mod "$FIXED"
record_blob crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs buffered "$FIXED"
stream_arrow=$(git -C "$SRC" rev-parse "${ARROW}:crates/oo-connectors/src/stream.rs")
stream_fixed=$(git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/src/stream.rs")
buffered_driver=$(git -C "$SRC" rev-parse "${DRIVER}:crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs")
buffered_fixed=$(git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs")
types_arrow=$(git -C "$SRC" rev-parse "${ARROW}:crates/oo-connectors/src/types/mod.rs")
types_fixed=$(git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/src/types/mod.rs")
error_arrow=$(git -C "$SRC" rev-parse "${ARROW}:crates/oo-connectors/src/error.rs")
error_fixed=$(git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/src/error.rs")
test "$stream_arrow" = "$stream_fixed"
test "$buffered_driver" = "$buffered_fixed"
test "$types_arrow" = "$types_fixed"
test "$error_arrow" = "$error_fixed"
printf 'stream_eq_75b993d0=%s\nbuffered_eq_bd77b252=%s\ntypes_mod_eq_75b993d0=%s\nerror_eq_75b993d0=%s\n' \
  "$stream_fixed" "$buffered_fixed" "$types_fixed" "$error_fixed" > "$EVIDENCE/composition-blobs.txt"

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology --label oo.owner=a137 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a137 --label oo.kind=cargo \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology --label oo.owner=a137 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a137 --label oo.kind=target \
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
tree=$(cat "$EVIDENCE/archive-tree.txt")
composition=exact dd5bf441 archive, includes Arrow 75b993d0 stream.rs and driver bd77b252 buffered.rs, no review overlay
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
  --label oo.owner=a137 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a137 \
  --label oo.task=arrow-acl-review-smoke \
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
