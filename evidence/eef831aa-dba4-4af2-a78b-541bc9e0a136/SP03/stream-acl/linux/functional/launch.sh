#!/bin/bash
# One owned linux/arm64 Rust 1.94 builder. Archive bd77b252 plus the ACL overlay only.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a136/SP03/stream-acl/linux/functional
ROOT=/tmp/oo-a136-eef831aa
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a136-msrv-acl
NET=oo-a136-eef831aa-net
CARGO_VOL=oo-a136-eef831aa-cargo
TARGET_VOL=oo-a136-eef831aa-target
PGLOG=oo-a136-eef831aa-pglog
FIXED=bd77b2520bcc3d63517bc49309002f25b58544b1
SNAP=${ROOT}/overlay
SEC=/tmp/oo-a136-eef831aa-dba4-4af2-a78b-541bc9e0a136/secrets

mkdir -p "$EVIDENCE"
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
test -f "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a136/SP03/stream-acl/linux/smoke/lib.exit"
test "$(cat "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a136/SP03/stream-acl/linux/smoke/lib.exit")" = "0"
test "$(cat "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a136/SP03/stream-acl/linux/smoke/clippy.exit")" = "0"
test "$(cat "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a136/SP03/stream-acl/pg/up.exit")" = "0"
docker inspect -f '{{.State.Status}}' oo-a136-pg | grep -q '^running$'

docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}' > "$EVIDENCE/containers-before.txt"
ps -axo pid=,ppid=,etime=,comm= > "$EVIDENCE/processes-comm-only.txt"
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a136 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
own_pg=$(docker ps -q --filter label=oo.owner=a136 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
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
cp "$SRC/crates/oo-connectors/src/snapshot.rs" "$SNAP/crates/oo-connectors/src/snapshot.rs"
cp "$SRC/crates/oo-connectors/src/sql.rs" "$SNAP/crates/oo-connectors/src/sql.rs"
cp "$SRC/crates/oo-connectors/tests/pg_stream_acl_live.rs" "$SNAP/crates/oo-connectors/tests/pg_stream_acl_live.rs"
cp "$SRC/contracts/source/pgsql/bounded-stream-v1.md" "$SNAP/contracts/source/pgsql/bounded-stream-v1.md"
stream_archived=$(git -C "$SRC" hash-object "$SNAP/crates/oo-connectors/src/stream.rs")
stream_fixed=$(git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/src/stream.rs")
test "$stream_archived" = "$stream_fixed"
live_archived=$(git -C "$SRC" hash-object "$SNAP/crates/oo-connectors/tests/pg_stream_live.rs")
live_fixed=$(git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/tests/pg_stream_live.rs")
test "$live_archived" = "$live_fixed"
printf '%s\n' "$stream_archived" > "$EVIDENCE/stream-blob.txt"
git -C "$SRC" hash-object "$SNAP/crates/oo-connectors/src/snapshot.rs" > "$EVIDENCE/snapshot-blob.txt"
git -C "$SRC" hash-object "$SNAP/crates/oo-connectors/src/sql.rs" > "$EVIDENCE/sql-blob.txt"
git -C "$SRC" hash-object "$SNAP/crates/oo-connectors/tests/pg_stream_acl_live.rs" > "$EVIDENCE/acl-test-blob.txt"
git -C "$SRC" hash-object "$SNAP/contracts/source/pgsql/bounded-stream-v1.md" > "$EVIDENCE/contract-blob.txt"

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
composition=fixed archive plus snapshot.rs sql.rs pg_stream_acl_live.rs bounded-stream-v1.md
stream.rs=${stream_fixed}
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
container=${NAME}
network=${NET}
pg_container=oo-a136-pg
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
  -e OO_PG_STREAM_ACL_OUTCOME=/evidence/acl-outcome.txt \
  --mount "type=bind,source=${SNAP},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --mount "type=volume,source=${PGLOG},target=/pglog,readonly" \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a136 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a136 \
  --label oo.task=stream-acl \
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
