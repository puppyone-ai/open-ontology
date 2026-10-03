#!/bin/bash
# One owned linux/arm64 Rust 1.94 functional builder.
# Compile base: git archive of the fixed SHA, plus only the owned overlay.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a131/SP03/arrow-stream/linux/functional
ROOT=/tmp/oo-a131-eef831aa
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a131-msrv-stream
NET=oo-a131-eef831aa-net
CARGO_VOL=oo-a131-eef831aa-cargo
TARGET_VOL=oo-a131-eef831aa-target
PGLOG=oo-a131-eef831aa-pglog
FIXED=ccd93e1adc34b24ab998d90972549f8cffb59f0d
SEC=/tmp/oo-a131-eef831aa-dba4-4af2-a78b-541bc9e0a131/secrets
SNAP=${ROOT}/functional-snapshot

mkdir -p "$EVIDENCE"
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"

test -f "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a131/SP03/arrow-stream/pg/up.exit"
test "$(cat "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a131/SP03/arrow-stream/pg/up.exit")" = "0"
docker inspect -f '{{.State.Status}}' oo-a131-pg | grep -q '^running$'

docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}' > "$EVIDENCE/containers-before.txt"
ps -axo pid=,ppid=,etime=,comm= > "$EVIDENCE/processes-comm-only.txt"
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a131 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
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

rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive "$FIXED" | tar -x -C "$SNAP"
overlay=(
  crates/oo-connectors/src/snapshot.rs
  crates/oo-connectors/src/stream.rs
  crates/oo-connectors/src/session.rs
  crates/oo-connectors/src/error.rs
  crates/oo-connectors/src/lib.rs
  crates/oo-connectors/src/sql.rs
  crates/oo-connectors/src/types/read.rs
  crates/oo-connectors/src/types/batch.rs
  crates/oo-connectors/src/types/mod.rs
  crates/oo-connectors/tests/pg_stream_live.rs
  crates/oo-connectors/tests/pg_stream_bounds.rs
  contracts/source/pgsql/bounded-stream-v1.md
)
for rel in "${overlay[@]}"; do
  cp "$SRC/$rel" "$SNAP/$rel"
done
printf '%s\n' "$FIXED" > "$EVIDENCE/archive-base.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/archive-tree.txt"
# The archive vendor is the fixed commit. Confirm the receive buffer was not replaced.
BUF=crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs
committed=$(git -C "$SRC" rev-parse "${FIXED}:${BUF}")
archived=$(git hash-object "$SNAP/$BUF")
printf 'committed=%s\narchived=%s\n' "$committed" "$archived" > "$EVIDENCE/buffered-blob.txt"
shasum -a 256 "$SNAP/$BUF" > "$EVIDENCE/buffered-archive-sha.txt"
test "$committed" = "$archived"

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
docker volume inspect "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null
docker volume inspect "$PGLOG" >/dev/null
test -f "$SEC/pointer.json"
test -f "$SEC/ca.crt"

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
overlay=owned stream files only
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
container=${NAME}
network=${NET}
pg_container=oo-a131-pg
pull=never
privileged=no
home_mount=no
socket_mount=no
host_port=none
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
  -e OO_PG_STREAM_OUTCOME=/evidence/live-outcome.txt \
  -e OO_PG_STREAM_BOUNDS_OUTCOME=/evidence/bounds-outcome.txt \
  --mount "type=bind,source=${SNAP},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --mount "type=volume,source=${PGLOG},target=/pglog,readonly" \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a131 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a131 \
  --label oo.task=arrow-stream-functional \
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
