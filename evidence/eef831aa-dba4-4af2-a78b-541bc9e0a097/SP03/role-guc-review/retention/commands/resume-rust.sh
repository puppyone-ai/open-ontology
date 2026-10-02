#!/bin/bash
# Restart only the owned Rust builder. The fixture, archive, and volumes stay.
set -eu
RUN=eef831aa-dba4-4af2-a78b-541bc9e0a097
ROOT=/Users/redux/puppy-one/open-ontology
BASE=/tmp/oo-$RUN
EVID=$ROOT/evidence/$RUN/SP03/role-guc-review/retention
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NET=oo-a097-ret-net-$RUN
PG_NAME=oo-a097-ret-pg-$RUN
RUST_NAME=oo-a097-ret-msrv-$RUN
CARGO_VOL=oo-a097-ret-cargo-$RUN
TARGET_VOL=oo-a097-ret-target-$RUN
PG_VOL=oo-a097-ret-pglog-$RUN

rust_running=$(docker ps --filter status=running --format '{{.Image}}' | awk '/0f24958fa693/ {n++} END{print n+0}')
host_running=$(ps -ax -o comm= | awk '$0 ~ /(^|\/)(cargo|rustc|rustdoc|clippy-driver)$/ {n++} END{print n+0}')
echo "rust_running_count=$rust_running host_rust_count=$host_running"
if [ "$((rust_running + host_running))" -ge 2 ]; then
  echo "builder capacity full" >&2
  exit 2
fi
pg_state=$(docker inspect -f '{{.State.Status}}' "$PG_NAME")
if [ "$pg_state" != "running" ]; then
  echo "fixture is not running" >&2
  exit 1
fi
if docker inspect "$RUST_NAME" >/dev/null 2>&1; then
  docker rm "$RUST_NAME" >/dev/null
fi

docker run -d --name "$RUST_NAME" \
  --platform linux/arm64 \
  --pull never \
  --cap-drop ALL \
  --security-opt no-new-privileges:true \
  --init \
  --memory 8g --memory-swap 8g --cpus 4 --pids-limit 4096 \
  --network "$NET" \
  --label project=open-ontology --label owner=a097 --label scope=role-guc-retention --label run="$RUN" \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_TERM_COLOR=never \
  -e OO_PG_FIXTURE=/secrets/pointer.json \
  -v "$BASE/isolated-archive":/src:ro \
  -v "$BASE/secrets":/secrets:ro \
  -v "$PG_VOL":/pglog:ro \
  -v "$EVID/rerun/phases":/phases \
  -v "$BASE/a097_retention_rerun.rs":/opt/a097/a097_retention_rerun.rs:ro \
  -v "$EVID/rerun/inside.sh":/inside.sh:ro \
  -v "$CARGO_VOL":/cargo \
  -v "$TARGET_VOL":/target \
  -w /src \
  "$RUST_IMAGE" \
  bash /inside.sh >/dev/null

echo "resume_ok name=$RUST_NAME"
