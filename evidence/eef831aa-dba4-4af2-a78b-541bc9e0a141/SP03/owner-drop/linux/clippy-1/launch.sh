#!/bin/bash
# Clippy gate only. Live tests already passed in linux/green-3 and are not repeated.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a141/SP03/owner-drop/linux/clippy-1
ROOT=/tmp/oo-a141-eef831aa
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a141-clippy-1
NET=oo-a141-eef831aa-net
CARGO_VOL=oo-a141-eef831aa-cargo
TARGET_VOL=oo-a141-eef831aa-target
FIXED=58afa97ded85a7dd89c72532bfc8973c45255d7c
TREE=543fd103532a8366ce92a0b4eaea4e089eb76d2d
SNAP=${ROOT}/clippy-1-src

mkdir -p "$EVIDENCE" "$ROOT"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite clippy-1 console.log" >&2
  exit 2
fi
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"

docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}' > "$EVIDENCE/containers-before.txt"
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a141 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
own_pg=$(docker ps -q --filter label=oo.owner=a141 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s own_postgres_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$own_pg" "$host_cargo" > "$EVIDENCE/capacity.txt"
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
if [ "$own_pg" -ne 1 ]; then
  echo "deferred: expected exactly one owned postgres" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi

actual_tree=$(git -C "$SRC" rev-parse "${FIXED}^{tree}")
test "$actual_tree" = "$TREE"
rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive "$FIXED" | tar -x -C "$SNAP"

same_as_fixed() {
  rel=$1
  base=$(git -C "$SRC" rev-parse "${FIXED}:${rel}")
  now=$(git -C "$SRC" hash-object "${SRC}/${rel}")
  test "$base" = "$now"
}
same_as_fixed crates/oo-connectors/src/stream.rs
same_as_fixed crates/oo-connectors/tests/pg_reader_cancel_live.rs
same_as_fixed crates/oo-connectors/tests/pg_reader_keys_live.rs

overlay_changed() {
  rel=$1
  name=$2
  cp "${SRC}/${rel}" "${SNAP}/${rel}"
  base=$(git -C "$SRC" rev-parse "${FIXED}:${rel}")
  now=$(git -C "$SRC" hash-object "${SNAP}/${rel}")
  printf 'base_blob=%s overlay_blob=%s\n' "$base" "$now" > "$EVIDENCE/${name}-blob.txt"
  test "$base" != "$now"
  shasum -a 256 "${SNAP}/${rel}" | awk '{print $1}' > "$EVIDENCE/expect-${name}.sha256"
  shasum -a 256 "${SRC}/${rel}" | awk '{print $1}' > "$EVIDENCE/workspace-${name}.sha256"
  cmp "$EVIDENCE/expect-${name}.sha256" "$EVIDENCE/workspace-${name}.sha256"
}
overlay_changed crates/oo-connectors/src/snapshot.rs snapshot
overlay_changed crates/oo-connectors/src/session.rs session
overlay_changed contracts/source/pgsql/reader-lifecycle-v1.md lifecycle
cp "$SRC/crates/oo-connectors/tests/pg_reader_drop_live.rs" "$SNAP/crates/oo-connectors/tests/pg_reader_drop_live.rs"
shasum -a 256 "$SNAP/crates/oo-connectors/tests/pg_reader_drop_live.rs" | awk '{print $1}' > "$EVIDENCE/expect-drop-test.sha256"
shasum -a 256 "$SRC/crates/oo-connectors/tests/pg_reader_drop_live.rs" | awk '{print $1}' > "$EVIDENCE/workspace-drop-test.sha256"
cmp "$EVIDENCE/expect-drop-test.sha256" "$EVIDENCE/workspace-drop-test.sha256"

stream_base=$(git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/src/stream.rs")
stream_arch=$(git -C "$SRC" hash-object "$SNAP/crates/oo-connectors/src/stream.rs")
test "$stream_base" = "$stream_arch"
shasum -a 256 "$SNAP/crates/oo-connectors/src/stream.rs" | awk '{print $1}' > "$EVIDENCE/expect-stream.sha256"
printf '%s\n' "$FIXED" > "$EVIDENCE/archive-base.txt"
printf '%s\n' "$actual_tree" > "$EVIDENCE/archive-tree.txt"

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
docker volume inspect "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null

if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named clippy-1 container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi

cat > "$EVIDENCE/invocation.txt" <<EOF
source_commit=${FIXED}
tree=${actual_tree}
composition=58afa97 archive plus snapshot.rs session.rs reader-lifecycle-v1.md pg_reader_drop_live.rs
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
offline_cargo=yes
phases=clippy
prior_live_tests=linux/green-3
clippy_component=rustup component add clippy --toolchain 1.94.0 when missing, same as linux/smoke
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
  --label oo.owner=a141 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a141 \
  --label oo.task=owner-drop-clippy-1 \
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
