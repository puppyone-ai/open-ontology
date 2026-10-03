#!/bin/bash
# Fixed archive. One drop, then cancel and keys once. Smoke and Clippy must already be 0.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
ROOT_EV=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a143/SP03/owner-drop-review
EVIDENCE=${ROOT_EV}/linux/probe-1
SMOKE=${ROOT_EV}/linux/smoke
ROOT=/tmp/oo-a143-eef831aa-dba4-4af2-a78b-541bc9e0a143
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a143-probe-1
NET=oo-a143-eef831aa-net
CARGO_VOL=oo-a143-eef831aa-cargo
TARGET_VOL=oo-a143-eef831aa-target
PGLOG=oo-a143-eef831aa-pglog
SEC=/tmp/oo-a143-eef831aa-dba4-4af2-a78b-541bc9e0a143/secrets
FIXED=6b95d6f2fd2259edfcbffc27a6338518a64dd0fc
BASE=58afa97ded85a7dd89c72532bfc8973c45255d7c
SNAP=${ROOT}/probe-1-src
HEAD_EXPECT=c83184f2f930435e17dd7c87d06b9d8ce7cc1554

mkdir -p "$EVIDENCE"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite probe-1 console.log" >&2
  exit 2
fi
test "$(cat "$SMOKE/lib.exit")" = "0"
test "$(cat "$SMOKE/clippy.exit")" = "0"
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
test -f "$SEC/pointer.json"
test -f "$SEC/ca.crt"
head_now=$(git -C "$SRC" rev-parse HEAD)
printf '%s\n' "$head_now" > "$EVIDENCE/workspace-head.txt"
test "$head_now" = "$HEAD_EXPECT"

docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}' > "$EVIDENCE/containers-before.txt"
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a143 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
own_pg=$(docker ps -q --filter label=oo.owner=a143 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
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
rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive "$FIXED" | tar -x -C "$SNAP"
head_after=$(git -C "$SRC" rev-parse HEAD)
test "$head_after" = "$HEAD_EXPECT"

blob_sha() { git -C "$SRC" rev-parse "${1}:${2}"; }
file_sha() { shasum -a 256 "$1" | awk '{print $1}'; }
match_blob() {
  rel=$1
  name=$2
  want=$(blob_sha "$FIXED" "$rel")
  got=$(git -C "$SRC" hash-object "$SNAP/$rel")
  test "$want" = "$got"
  file_sha "$SNAP/$rel" > "$EVIDENCE/expect-${name}.sha256"
}
match_blob crates/oo-connectors/src/snapshot.rs snapshot
match_blob crates/oo-connectors/src/session.rs session
match_blob crates/oo-connectors/src/stream.rs stream
match_blob crates/oo-connectors/tests/pg_reader_drop_live.rs drop-test
match_blob contracts/source/pgsql/reader-lifecycle-v1.md lifecycle
test "$(blob_sha "$BASE" crates/oo-connectors/src/stream.rs)" = "$(blob_sha "$FIXED" crates/oo-connectors/src/stream.rs)"
printf '%s\n' "$FIXED" > "$EVIDENCE/archive-base.txt"
printf '%s\n' "$actual_tree" > "$EVIDENCE/archive-tree.txt"

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
docker volume inspect "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null
docker volume inspect "$PGLOG" >/dev/null
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named probe-1 container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi

cat > "$EVIDENCE/invocation.txt" <<EOF
source_commit=${FIXED}
tree=${actual_tree}
composition=git archive of ${FIXED}
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
container=${NAME}
network=${NET}
pg_container=oo-a143-pg
pull=never
privileged=no
home_mount=no
socket_mount=no
host_network=no
shared_cargo_cache=no
offline=yes
phases=pg_reader_drop_live pg_reader_cancel_live pg_reader_keys_live
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
  -e OO_PG_DROP_FIXTURE=/secrets/pointer.json \
  -e OO_PG_DROP_OUTCOME=/evidence/drop-outcome.txt \
  -e OO_PG_CANCEL_FIXTURE=/secrets/pointer.json \
  -e OO_PG_CANCEL_OUTCOME=/evidence/cancel-outcome.txt \
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
  --label oo.owner=a143 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a143 \
  --label oo.task=owner-drop-review-probe-1 \
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
