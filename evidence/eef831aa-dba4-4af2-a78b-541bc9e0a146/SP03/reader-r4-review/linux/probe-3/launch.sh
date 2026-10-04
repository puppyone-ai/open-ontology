#!/bin/bash
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a146/SP03/reader-r4-review/linux/probe-3
ROOT=/tmp/oo-a146-eef831aa-dba4-4af2-a78b-541bc9e0a146
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a146-probe-3
NET=oo-a146-eef831aa-net
CARGO_VOL=oo-a146-eef831aa-cargo
TARGET_VOL=oo-a146-eef831aa-target
LOGVOL=oo-a146-eef831aa-pglog
FIXED=f5f0a73b0678e8b1611f1dc4a40e1dbf1d67b10e
EXPECT_TREE=6ec4bf24dd0111089b87b9b805e2ee71837cd34a
SNAP=${ROOT}/probe3-src
SEC=${ROOT}/secrets
PROBE_SRC=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a146/SP03/reader-r4-review/overlay/pg_reader_r4_probe.rs
mkdir -p "$EVIDENCE"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite probe-3 console.log" >&2
  exit 2
fi
test -f "$SEC/pointer.json"
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
head_now=$(git -C "$SRC" rev-parse HEAD)
printf '%s\n' "$head_now" > "$EVIDENCE/workspace-head.txt"
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a146 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$((running_rust + host_cargo))" -ge 2 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
docker exec -u postgres oo-a146-pg bash -c ': > /pglog/postgresql.log'
actual_tree=$(git -C "$SRC" rev-parse "${FIXED}^{tree}")
test "$actual_tree" = "$EXPECT_TREE"
printf '%s\n' "$FIXED" > "$EVIDENCE/archive-base.txt"
printf '%s\n' "$actual_tree" > "$EVIDENCE/archive-tree.txt"
rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive "$FIXED" | tar -x -C "$SNAP"
cp "$PROBE_SRC" "$SNAP/crates/oo-connectors/tests/pg_reader_r4_probe.rs"
file_sha() { shasum -a 256 "$1" | awk '{print $1}'; }
expect_file() {
  rel=$1
  name=$2
  want=$(git -C "$SRC" rev-parse "${FIXED}:${rel}")
  got=$(git -C "$SRC" hash-object "$SNAP/$rel")
  test "$want" = "$got"
  printf '%s\n' "$want" > "$EVIDENCE/blob-${name}.txt"
  file_sha "$SNAP/$rel" > "$EVIDENCE/expect-${name}.sha256"
}
expect_file crates/oo-connectors/src/snapshot.rs snapshot
expect_file crates/oo-connectors/src/session.rs session
expect_file crates/oo-connectors/src/stream.rs stream
expect_file contracts/source/pgsql/reader-lifecycle-v1.md lifecycle
file_sha "$SNAP/crates/oo-connectors/tests/pg_reader_r4_probe.rs" > "$EVIDENCE/expect-probe.sha256"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named probe-3 container already exists" >&2
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
  --mount "type=bind,source=${SNAP},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=volume,source=${LOGVOL},target=/pglog,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a146 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a146 \
  --label oo.task=reader-r4-probe-3 \
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
} > "$EVIDENCE/hostconfig.txt"
exit "$run_exit"
