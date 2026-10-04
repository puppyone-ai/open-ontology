#!/bin/bash
# Immutable archive of fixed base 4ed22b8c plus the activation overlay. One rust builder.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a151/SP03/reader-activation/linux/probe
ROOT=/tmp/oo-a151-eef831aa-dba4-4af2-a78b-541bc9e0a151
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a151-probe
NET=oo-a151-eef831aa-net
CARGO_VOL=oo-a151-eef831aa-cargo
TARGET_VOL=oo-a151-eef831aa-target
LOGVOL=oo-a151-eef831aa-pglog
FIXED=4ed22b8c1189e72b06b20a45335860ac9a3ea44e
EXPECT_TREE=fafb0fe6752b8240eb2b29025fc38bccdfb7144d
SNAP=${ROOT}/probe-src
SEC=${ROOT}/secrets

mkdir -p "$EVIDENCE"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite probe console.log" >&2
  exit 2
fi
test -f "$SEC/pointer.json"
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if docker container inspect oo-a151-smoke >/dev/null 2>&1; then
  state=$(docker inspect -f '{{.State.Status}}' oo-a151-smoke)
  if [ "$state" != "exited" ]; then
    echo "smoke container is still $state" >&2
    printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
    exit 3
  fi
  docker rm oo-a151-smoke >/dev/null
fi
head_now=$(git -C "$SRC" rev-parse HEAD)
printf '%s\n' "$head_now" > "$EVIDENCE/workspace-head.txt"
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a151 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$((running_rust + host_cargo))" -ge 2 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  echo "private network missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
actual_tree=$(git -C "$SRC" rev-parse "${FIXED}^{tree}")
test "$actual_tree" = "$EXPECT_TREE"
printf '%s\n' "$FIXED" > "$EVIDENCE/archive-base.txt"
printf '%s\n' "$actual_tree" > "$EVIDENCE/archive-tree.txt"
git -C "$SRC" diff --stat HEAD -- crates/oo-connectors contracts/source/pgsql > "$EVIDENCE/diff-stat.txt"
rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive "$FIXED" | tar -x -C "$SNAP"
file_sha() { shasum -a 256 "$1" | awk '{print $1}'; }
file_sha "$SNAP/crates/oo-connectors/src/snapshot.rs" > "$EVIDENCE/expect-snapshot.sha256"
file_sha "$SNAP/crates/oo-connectors/src/stream.rs" > "$EVIDENCE/expect-stream.sha256"
file_sha "$SNAP/crates/oo-connectors/src/session.rs" > "$EVIDENCE/expect-session.sha256"
file_sha "$SNAP/crates/oo-connectors/src/sql.rs" > "$EVIDENCE/expect-sql.sha256"
file_sha "$SNAP/crates/oo-connectors/Cargo.toml" > "$EVIDENCE/expect-cargo-toml.sha256"
file_sha "$SNAP/Cargo.lock" > "$EVIDENCE/expect-cargo-lock.sha256"
overlay() {
  rel=$1
  cp "$SRC/$rel" "$SNAP/$rel"
}
overlay crates/oo-connectors/src/model.rs
overlay crates/oo-connectors/src/types/mod.rs
overlay crates/oo-connectors/src/types/tests.rs
overlay crates/oo-connectors/src/key_plan.rs
overlay crates/oo-connectors/src/lib.rs
overlay crates/oo-connectors/tests/live_pg.rs
overlay crates/oo-connectors/tests/lossless_live.rs
overlay crates/oo-connectors/tests/pg_reader_complete_live.rs
overlay contracts/source/pgsql/bounded-stream-v1.md
overlay contracts/source/pgsql/reader-lifecycle-v1.md
file_sha "$SNAP/crates/oo-connectors/src/model.rs" > "$EVIDENCE/expect-model.sha256"
file_sha "$SNAP/crates/oo-connectors/src/types/mod.rs" > "$EVIDENCE/expect-types.sha256"
file_sha "$SNAP/crates/oo-connectors/src/types/tests.rs" > "$EVIDENCE/expect-types-tests.sha256"
file_sha "$SNAP/crates/oo-connectors/src/key_plan.rs" > "$EVIDENCE/expect-keyplan.sha256"
file_sha "$SNAP/crates/oo-connectors/src/lib.rs" > "$EVIDENCE/expect-lib.sha256"
file_sha "$SNAP/crates/oo-connectors/tests/live_pg.rs" > "$EVIDENCE/expect-live.sha256"
file_sha "$SNAP/crates/oo-connectors/tests/lossless_live.rs" > "$EVIDENCE/expect-lossless.sha256"
file_sha "$SNAP/crates/oo-connectors/tests/pg_reader_complete_live.rs" > "$EVIDENCE/expect-complete.sha256"
file_sha "$SNAP/contracts/source/pgsql/bounded-stream-v1.md" > "$EVIDENCE/expect-bounded.sha256"
file_sha "$SNAP/contracts/source/pgsql/reader-lifecycle-v1.md" > "$EVIDENCE/expect-lifecycle.sha256"
{
  echo "model $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/src/model.rs")"
  echo "types_mod $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/src/types/mod.rs")"
  echo "types_tests $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/src/types/tests.rs")"
  echo "key_plan $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/src/key_plan.rs")"
  echo "lib $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/src/lib.rs")"
  echo "live_pg $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/tests/live_pg.rs")"
  echo "lossless $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/tests/lossless_live.rs")"
  echo "complete $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/tests/pg_reader_complete_live.rs")"
  echo "lifecycle $(git -C "$SRC" rev-parse "${FIXED}:contracts/source/pgsql/reader-lifecycle-v1.md")"
  echo "bounded $(git -C "$SRC" rev-parse "${FIXED}:contracts/source/pgsql/bounded-stream-v1.md")"
  echo "snapshot $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/src/snapshot.rs")"
  echo "stream $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/src/stream.rs")"
  echo "session $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/src/session.rs")"
  echo "sql $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/src/sql.rs")"
  echo "cargo_toml $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/Cargo.toml")"
  echo "cargo_lock $(git -C "$SRC" rev-parse "${FIXED}:Cargo.lock")"
} > "$EVIDENCE/fixed-blobs.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named probe container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
docker exec -u postgres oo-a151-pg bash -c ': > /pglog/postgresql.log'
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
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=volume,source=${LOGVOL},target=/pglog" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a151 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a151 \
  --label oo.task=reader-activation-probe \
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
