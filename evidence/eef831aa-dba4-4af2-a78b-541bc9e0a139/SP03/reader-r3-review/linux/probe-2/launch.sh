#!/bin/bash
# One owned linux/arm64 Rust 1.94 builder. 58afa97 plus the review test only.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
ROOT_EV=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a139/SP03/reader-r3-review
EVIDENCE=${ROOT_EV}/linux/probe-2
AUTHOR=${ROOT_EV}/linux/author
SMOKE=${ROOT_EV}/linux/smoke
PG=${ROOT_EV}/pg
OVERLAY=${ROOT_EV}/overlay/pg_reader_r3_review.rs
TMP=/tmp/oo-a139-eef831aa
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a139-msrv-probe2
NET=oo-a139-eef831aa-net
CARGO_VOL=oo-a139-eef831aa-cargo
TARGET_VOL=oo-a139-eef831aa-target
PGLOG=oo-a139-eef831aa-pglog
FIXED=58afa97ded85a7dd89c72532bfc8973c45255d7c
TREE=543fd103532a8366ce92a0b4eaea4e089eb76d2d
SNAP=${TMP}/probe-src
SEC=/tmp/oo-a139-eef831aa-dba4-4af2-a78b-541bc9e0a139/secrets

mkdir -p "$EVIDENCE"
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -e "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite an existing probe console.log" >&2
  exit 2
fi
test -f "$SMOKE/lib.exit"
test "$(cat "$SMOKE/lib.exit")" = "0"
test "$(cat "$SMOKE/clippy.exit")" = "0"
test -f "$AUTHOR/keys.exit"
test "$(cat "$AUTHOR/keys.exit")" = "0"
test "$(cat "$AUTHOR/cancel.exit")" = "0"
test -f "${ROOT_EV}/linux/probe/review.exit"
test "$(cat "${ROOT_EV}/linux/probe/review.exit")" != "0"
test -f "$PG/up.exit"
test "$(cat "$PG/up.exit")" = "0"
docker inspect -f '{{.State.Status}}' oo-a139-pg | grep -q '^running$'
test -f "$OVERLAY"

docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}' > "$EVIDENCE/containers-before.txt"
ps -axo pid=,ppid=,etime=,comm= > "$EVIDENCE/processes-comm-only.txt"
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a139 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
own_pg=$(docker ps -q --filter label=oo.owner=a139 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
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

actual_tree=$(git -C "$SRC" rev-parse "${FIXED}^{tree}")
test "$actual_tree" = "$TREE"
rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive "$FIXED" | tar -x -C "$SNAP"
cp "$OVERLAY" "$SNAP/crates/oo-connectors/tests/pg_reader_r3_review.rs"
printf '%s\n' "$FIXED" > "$EVIDENCE/archive-base.txt"
printf '%s\n' "$actual_tree" > "$EVIDENCE/archive-tree.txt"
test -f "$SNAP/crates/oo-connectors/src/key_plan.rs"
test -f "$SNAP/crates/oo-connectors/tests/pg_reader_keys_live.rs"
test -f "$SNAP/crates/oo-connectors/tests/pg_reader_r3_review.rs"
check_blob() {
  rel=$1
  committed=$(git -C "$SRC" rev-parse "${FIXED}:${rel}")
  archived=$(git -C "$SRC" hash-object "$SNAP/$rel")
  test "$committed" = "$archived"
}
check_blob crates/oo-connectors/src/key_plan.rs
check_blob crates/oo-connectors/src/snapshot.rs
check_blob crates/oo-connectors/src/sql.rs
check_blob crates/oo-connectors/src/stream.rs
check_blob crates/oo-connectors/src/error.rs
check_blob crates/oo-connectors/src/session.rs
check_blob crates/oo-connectors/src/config.rs
check_blob crates/oo-connectors/src/lib.rs
check_blob crates/oo-connectors/src/types/mod.rs
check_blob crates/oo-connectors/src/types/read.rs
check_blob crates/oo-connectors/tests/pg_reader_keys_live.rs
check_blob crates/oo-connectors/tests/pg_reader_cancel_live.rs
cmp -s "$OVERLAY" "$SNAP/crates/oo-connectors/tests/pg_reader_r3_review.rs"
shasum -a 256 "$OVERLAY" | awk '{print $1}' > "$EVIDENCE/overlay.sha256"
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/workspace-head.txt"

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
tree=${actual_tree}
keys_commit=8bda0cd48976b23d268d125b34bc8130d94b8253
composition=exact 58afa97 archive plus overlay/pg_reader_r3_review.rs only
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
container=${NAME}
network=${NET}
pg_container=oo-a139-pg
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
  -e OO_PG_REVIEW_FIXTURE=/secrets/pointer.json \
  -e OO_PG_REVIEW_OUTCOME=/evidence/review-outcome.txt \
  --mount "type=bind,source=${SNAP},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --mount "type=volume,source=${PGLOG},target=/pglog,readonly" \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a139 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a139 \
  --label oo.task=reader-r3-review-probe2 \
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
