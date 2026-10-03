#!/bin/bash
# One owned rust builder for the column-grant representative. Postgres is already up.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a137/SP03/arrow-acl-review/linux/column-grant
ROOT=/tmp/oo-a137-eef831aa
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a137-msrv-colg
NET=oo-a137-eef831aa-net
CARGO_VOL=oo-a137-eef831aa-cargo
TARGET_VOL=oo-a137-eef831aa-target
PGLOG=oo-a137-eef831aa-pglog
FIXED=dd5bf44160f949c0ac21a0ce3ddf5ee5e90b4200
SNAP=${ROOT}/column-grant
SEC=/tmp/oo-a137-eef831aa-dba4-4af2-a78b-541bc9e0a137/secrets

mkdir -p "$EVIDENCE"
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
docker inspect -f '{{.State.Status}}' oo-a137-pg | grep -q '^running$'
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a137 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$((running_rust + host_cargo))" -ge 2 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive "$FIXED" | tar -x -C "$SNAP"
cp "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a137/SP03/arrow-acl-review/overlay/pg_stream_acl_review.rs" \
  "$SNAP/crates/oo-connectors/tests/pg_stream_acl_review.rs"
test "$(git -C "$SRC" hash-object "$SNAP/crates/oo-connectors/src/sql.rs")" = "$(git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/src/sql.rs")"
test "$(git -C "$SRC" hash-object "$SNAP/crates/oo-connectors/src/snapshot.rs")" = "$(git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/src/snapshot.rs")"
test "$(git -C "$SRC" hash-object "$SNAP/crates/oo-connectors/src/stream.rs")" = "$(git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/src/stream.rs")"
git -C "$SRC" hash-object "$SNAP/crates/oo-connectors/tests/pg_stream_acl_review.rs" > "$EVIDENCE/review-test-blob.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  prior=$(docker inspect -f '{{.State.Status}}' "$NAME")
  if [ "$prior" = "running" ]; then
    echo "deferred: named container is already running" >&2
    printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
    exit 3
  fi
  docker rm "$NAME" >/dev/null
fi
cat > "$EVIDENCE/invocation.txt" <<EOF
source_commit=${FIXED}
composition=exact dd5bf441 archive plus the column-grant overlay only
container=${NAME}
network=${NET}
pg_container=oo-a137-pg
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
  -e OO_PG_STREAM_FIXTURE=/secrets/pointer.json \
  -e OO_PG_STREAM_COLG_OUTCOME=/evidence/colg-outcome.txt \
  --mount "type=bind,source=${SNAP},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --mount "type=volume,source=${PGLOG},target=/pglog,readonly" \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a137 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a137 \
  --label oo.task=arrow-acl-column-grant \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
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
