#!/bin/bash
# One owned linux/arm64 Rust 1.94 builder. Exact dd5bf441 plus the keys overlay.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a138/SP03/reader-r3/linux/keys
SMOKE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a138/SP03/reader-r3/linux/smoke
PG=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a138/SP03/reader-r3/pg
ROOT=/tmp/oo-a138-eef831aa
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a138-msrv-keys
NET=oo-a138-eef831aa-net
CARGO_VOL=oo-a138-eef831aa-cargo
TARGET_VOL=oo-a138-eef831aa-target
PGLOG=oo-a138-eef831aa-pglog
FIXED=dd5bf44160f949c0ac21a0ce3ddf5ee5e90b4200
SNAP=${ROOT}/keys
SEC=/tmp/oo-a138-eef831aa-dba4-4af2-a78b-541bc9e0a138/secrets

mkdir -p "$EVIDENCE"
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
test -f "$SMOKE/lib.exit"
test "$(cat "$SMOKE/lib.exit")" = "0"
test "$(cat "$SMOKE/clippy.exit")" = "0"
test -f "$PG/up.exit"
test "$(cat "$PG/up.exit")" = "0"
docker inspect -f '{{.State.Status}}' oo-a138-pg | grep -q '^running$'

docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}' > "$EVIDENCE/containers-before.txt"
ps -axo pid=,ppid=,etime=,comm= > "$EVIDENCE/processes-comm-only.txt"
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a138 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
own_pg=$(docker ps -q --filter label=oo.owner=a138 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s own_postgres_running=%s\n' "$running_rust" "$own" "$host_cargo" "$own_pg" > "$EVIDENCE/capacity.txt"
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
if [ "$own_pg" -ge 2 ]; then
  echo "deferred: more than one owned postgres" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi

rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive "$FIXED" | tar -x -C "$SNAP"
overlay() {
  rel=$1
  mkdir -p "$SNAP/$(dirname "$rel")"
  cp "$SRC/$rel" "$SNAP/$rel"
}
overlay crates/oo-connectors/src/snapshot.rs
overlay crates/oo-connectors/src/sql.rs
overlay crates/oo-connectors/src/stream.rs
overlay crates/oo-connectors/src/lib.rs
overlay crates/oo-connectors/src/key_plan.rs
overlay crates/oo-connectors/src/types/read.rs
overlay crates/oo-connectors/src/types/mod.rs
overlay crates/oo-connectors/tests/pg_reader_keys_live.rs
test -f "$SNAP/crates/oo-connectors/src/key_plan.rs"
# Unrelated sources stay the fixed archive.
test "$(git -C "$SRC" hash-object "$SNAP/crates/oo-connectors/src/session.rs")" = "$(git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/src/session.rs")"
test "$(git -C "$SRC" hash-object "$SNAP/crates/oo-connectors/src/error.rs")" = "$(git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/src/error.rs")"
test "$(git -C "$SRC" hash-object "$SNAP/crates/oo-connectors/src/config.rs")" = "$(git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/src/config.rs")"
git -C "$SRC" hash-object "$SNAP/crates/oo-connectors/src/key_plan.rs" > "$EVIDENCE/key-plan-blob.txt"
git -C "$SRC" hash-object "$SNAP/crates/oo-connectors/tests/pg_reader_keys_live.rs" > "$EVIDENCE/keys-test-blob.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/archive-base.txt"

test -f "$SEC/pointer.json"
test -f "$SEC/ca.crt"
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"

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
composition=exact dd5bf441 archive plus keys overlay
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
container=${NAME}
network=${NET}
pg_container=oo-a138-pg
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
  -e OO_PG_KEYS_FIXTURE=/secrets/pointer.json \
  -e OO_PG_KEYS_OUTCOME=/evidence/keys-outcome.txt \
  --mount "type=bind,source=${SNAP},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --mount "type=volume,source=${PGLOG},target=/pglog,readonly" \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a138 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a138 \
  --label oo.task=reader-r3-keys \
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
