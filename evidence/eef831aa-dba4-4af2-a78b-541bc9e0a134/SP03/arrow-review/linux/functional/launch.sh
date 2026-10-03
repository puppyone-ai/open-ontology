#!/bin/bash
# Functional builder. Reuses this task's cargo and target volumes. One PG already up.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a134/SP03/arrow-review/linux/functional
SMOKE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a134/SP03/arrow-review/linux/smoke
ROOT=/tmp/oo-a134-eef831aa
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a134-msrv-review
NET=oo-a134-eef831aa-net
CARGO_VOL=oo-a134-eef831aa-cargo
TARGET_VOL=oo-a134-eef831aa-target
PGLOG=oo-a134-eef831aa-pglog
SEC=/tmp/oo-a134-eef831aa-dba4-4af2-a78b-541bc9e0a134/secrets
SNAP=${ROOT}/snapshot
FIXED=75b993d0b578fa863a32c64abc0073ed92d51a5d

mkdir -p "$EVIDENCE"
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
test -f "$SMOKE/lib.exit"
test "$(cat "$SMOKE/lib.exit")" = "0"
test -f "$SMOKE/clippy.exit"
test "$(cat "$SMOKE/clippy.exit")" = "0"
test -f "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a134/SP03/arrow-review/pg/up.exit"
test "$(cat "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a134/SP03/arrow-review/pg/up.exit")" = "0"
docker inspect -f '{{.State.Status}}' oo-a134-pg | grep -q '^running$'

docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}' > "$EVIDENCE/containers-before.txt"
ps -axo pid=,ppid=,etime=,comm= > "$EVIDENCE/processes-comm-only.txt"
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a134 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
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

test -d "$SNAP"
test ! -f "$SNAP/crates/oo-connectors/tests/pg_stream_review.rs"
cp "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a134/SP03/arrow-review/overlay/pg_stream_review.rs" \
  "$SNAP/crates/oo-connectors/tests/pg_stream_review.rs"
shasum -a 256 "$SNAP/crates/oo-connectors/src/stream.rs" | awk '{print $1}' > "$EVIDENCE/expect-stream.sha256"
shasum -a 256 "$SNAP/crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs" | awk '{print $1}' > "$EVIDENCE/expect-buffered.sha256"
cmp "$EVIDENCE/expect-buffered.sha256" "$SMOKE/expect-buffered.sha256"
git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/src/stream.rs" > "$EVIDENCE/stream-blob.txt"
archived_stream=$(git -C "$SRC" hash-object "$SNAP/crates/oo-connectors/src/stream.rs")
test "$archived_stream" = "$(cat "$EVIDENCE/stream-blob.txt")"

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
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
composition=fixed commit plus review test overlay only
author_build=not this run
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
container=${NAME}
network=${NET}
pg_container=oo-a134-pg
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
  -e OO_PG_STREAM_FIXTURE=/secrets/pointer.json \
  -e OO_PG_STREAM_OUTCOME=/evidence/live-outcome.txt \
  -e OO_PG_STREAM_BOUNDS_OUTCOME=/evidence/bounds-outcome.txt \
  -e OO_PG_REVIEW_OUTCOME=/evidence/review-outcome.txt \
  --mount "type=bind,source=${SNAP},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --mount "type=volume,source=${PGLOG},target=/pglog,readonly" \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a134 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a134 \
  --label oo.task=arrow-review-functional \
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
