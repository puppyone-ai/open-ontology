#!/bin/bash
# Fixed archive plus the independent review test. One rust builder.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
BASE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a149/SP03/reader-completion-review
EVIDENCE=${BASE}/linux/probe
ROOT=/tmp/oo-a149-eef831aa-dba4-4af2-a78b-541bc9e0a149
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a149-probe
NET=oo-a149-eef831aa-net
CARGO_VOL=oo-a149-eef831aa-cargo
TARGET_VOL=oo-a149-eef831aa-target
LOGVOL=oo-a149-eef831aa-pglog
FIXED=16ca2969cd449653a64d98d9831d3c8b8976942a
EXPECT_TREE=0466e01e2794716f1d6bde65ba4ea82d39b88d1b
SNAP=${ROOT}/probe-src
SEC=${ROOT}/secrets

mkdir -p "$EVIDENCE"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite probe console.log" >&2
  exit 2
fi
test -f "$SEC/pointer.json"
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
head_now=$(git -C "$SRC" rev-parse HEAD)
printf '%s\n' "$head_now" > "$EVIDENCE/workspace-head.txt"
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a149 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$((running_rust + host_cargo))" -ge 2 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
docker exec -u postgres oo-a149-pg bash -c ': > /pglog/postgresql.log'
actual_tree=$(git -C "$SRC" rev-parse "${FIXED}^{tree}")
test "$actual_tree" = "$EXPECT_TREE"
printf '%s\n' "$FIXED" > "$EVIDENCE/archive-base.txt"
printf '%s\n' "$actual_tree" > "$EVIDENCE/archive-tree.txt"
rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive "$FIXED" | tar -x -C "$SNAP"
cp "${BASE}/overlay/pg_reader_r4_review.rs" "$SNAP/crates/oo-connectors/tests/pg_reader_r4_review.rs"
file_sha() { shasum -a 256 "$1" | awk '{print $1}'; }
file_sha "$SNAP/crates/oo-connectors/src/snapshot.rs" > "$EVIDENCE/expect-snapshot.sha256"
file_sha "$SNAP/crates/oo-connectors/src/stream.rs" > "$EVIDENCE/expect-stream.sha256"
file_sha "$SNAP/crates/oo-connectors/src/lib.rs" > "$EVIDENCE/expect-lib.sha256"
file_sha "$SNAP/crates/oo-connectors/src/session.rs" > "$EVIDENCE/expect-session.sha256"
file_sha "$SNAP/contracts/source/pgsql/reader-lifecycle-v1.md" > "$EVIDENCE/expect-lifecycle.sha256"
file_sha "$SNAP/contracts/source/pgsql/bounded-stream-v1.md" > "$EVIDENCE/expect-bounded.sha256"
file_sha "$SNAP/crates/oo-connectors/tests/pg_reader_r4_review.rs" > "$EVIDENCE/expect-review-test.sha256"
test "$(file_sha "${BASE}/overlay/pg_reader_r4_review.rs")" = "$(cat "$EVIDENCE/expect-review-test.sha256")"
{
  echo "snapshot $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/src/snapshot.rs")"
  echo "stream $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/src/stream.rs")"
  echo "session $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/src/session.rs")"
  echo "lib $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/src/lib.rs")"
  echo "lifecycle $(git -C "$SRC" rev-parse "${FIXED}:contracts/source/pgsql/reader-lifecycle-v1.md")"
  echo "bounded $(git -C "$SRC" rev-parse "${FIXED}:contracts/source/pgsql/bounded-stream-v1.md")"
} > "$EVIDENCE/fixed-blobs.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named probe container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
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
  --mount "type=bind,source=${SNAP},target=/src" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${BASE}/overlay,target=/overlay,readonly" \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=volume,source=${LOGVOL},target=/pglog,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a149 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a149 \
  --label oo.task=reader-completion-review-probe \
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
  echo "CapDrop=$(docker inspect -f '{{.HostConfig.CapDrop}}' "$NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$EVIDENCE/hostconfig.txt"
exit "$run_exit"
