#!/bin/bash
# One builder. Focused test and Clippy only. Fresh bind path. No secrets.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a401/SP09/local-analysis-review
LIVE=${REVIEW}/linux/proof
ROOT=/tmp/oo-a401-eef831aa-dba4-4af2-a78b-541bc9e0a401
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a401-proof
NET=oo-a401-eef831aa-net
CARGO_VOL=oo-a401-eef831aa-cargo
TARGET_VOL=oo-a401-eef831aa-target
NATIVE_VOL=oo-a401-eef831aa-native

if [ ! -f "$ROOT/analysis-archive/tools/local-analysis/src/lib.rs" ]; then
  echo "analysis archive is missing" >&2
  printf '%s\n' 2 > "$LIVE/docker-run.exit"
  exit 2
fi
cp "$REVIEW/linux/focused/a401_bounds.rs" "$ROOT/analysis-archive/tools/local-analysis/tests/a401_bounds.rs"
if [ -f "$LIVE/console-clippy.log" ]; then
  echo "refusing to overwrite console-clippy.log" >&2
  printf '%s\n' 2 > "$LIVE/docker-run.exit"
  exit 2
fi
ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}' > "$LIVE/host-cargo-count-clippy.txt"
host_n=$(cat "$LIVE/host-cargo-count-clippy.txt")
docker ps --filter status=running --filter label=oo.kind=rust-builder --format '{{.Names}}' > "$LIVE/builders-before-clippy.txt" || true
builder_n=$(grep -c . "$LIVE/builders-before-clippy.txt" || true)
if [ "$builder_n" -ge 10 ] || [ "$host_n" -ge 1 ]; then
  echo "deferred: rust builder capacity host=$host_n builders=$builder_n" >&2
  printf '%s\n' 3 > "$LIVE/docker-run.exit"
  exit 3
fi
own=$(docker ps -q --filter status=running --filter label=oo.owner=a401 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$LIVE/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named proof container already exists" >&2
  printf '%s\n' 2 > "$LIVE/docker-run.exit"
  exit 2
fi

STAGE="$ROOT/stage-clippy-$(date -u +%Y%m%dT%H%M%S)"
mkdir -p "$STAGE"
cp -a "$ROOT/analysis-archive" "$STAGE/analysis-archive"
ANALYSIS_MNT="$STAGE/analysis-archive/tools/local-analysis"
printf '%s\n' "$STAGE" > "$LIVE/clippy-stage.txt"
sync
sleep 1

set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 4096 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target/a401 \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${ANALYSIS_MNT},target=/local-analysis,readonly" \
  --mount "type=bind,source=${LIVE}/clippy-inside.sh,target=/clippy-inside.sh,readonly" \
  --mount "type=bind,source=${REVIEW},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --mount "type=volume,source=${NATIVE_VOL},target=/native" \
  --label oo.project=open-ontology \
  --label oo.owner=a401 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a401 \
  --label oo.task=sp09-local-analysis-review \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /clippy-inside.sh > "$LIVE/console-clippy.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$LIVE/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME" 2>/dev/null || echo missing)"
} > "$LIVE/hostconfig-clippy.txt"
docker rm "$NAME" > "$LIVE/container-removed-clippy.txt" 2>&1 || true
exit "$run_exit"
