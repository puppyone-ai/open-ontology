#!/bin/bash
# One linux/arm64 Rust 1.94 builder. PostgreSQL must already be the only other builder.
set -euo pipefail
umask 077
EV=/Users/redux/puppy-one/open-ontology/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a110/SP03/lossless-types-review
SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a110
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
RUN_ID=$(cat "$EV/raw/admin/run-id.txt")
ROOT=$(cat "$EV/raw/admin/secret-root.txt")
NET="oo-a110-pg-${RUN_ID}-net"
NAME="oo-a110-msrv-${RUN_ID}"
CARGO_VOL="oo-a110-cargo-${RUN_ID}"
TARGET_VOL="oo-a110-target-${RUN_ID}"

host_compilers=$(ps -axo comm= | awk '$1=="rustc" || $1=="cargo" || $1 ~ /^cargo-/ { c++ } END { print c+0 }')
running=$(docker ps -q | wc -l | tr -d ' ')
printf 'host_compilers=%s\nrunning_containers=%s\n' "$host_compilers" "$running" | tee "$EV/raw/admin/capacity-before-rust.txt"
if [ "$host_compilers" -ne 0 ] || [ "$running" -ne 1 ]; then
  echo "rust phase needs exactly the owned postgres fixture and no host compiler" >&2
  docker ps --format '{{.Names}} {{.Status}}' >&2 || true
  exit 3
fi
own=$(docker ps --filter label=oo.owner=a110 --filter label=oo.kind=postgres --format '{{.Names}}')
if [ "$own" != "oo-a110-pg-${RUN_ID}" ]; then
  echo "owned postgres fixture is not the running container: $own" >&2
  exit 3
fi
if ! docker image inspect "$RUST_IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  exit 1
fi

docker volume create \
  --label oo.project=open-ontology --label oo.owner=a110 --label oo.session="$SESSION" \
  --label oo.run_id="$RUN_ID" --label oo.kind=rust \
  "$CARGO_VOL" > "$EV/raw/admin/cargo-volume.txt"
docker volume create \
  --label oo.project=open-ontology --label oo.owner=a110 --label oo.session="$SESSION" \
  --label oo.run_id="$RUN_ID" --label oo.kind=rust \
  "$TARGET_VOL" > "$EV/raw/admin/target-volume.txt"

docker run -d \
  --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --cap-drop ALL \
  --security-opt no-new-privileges:true \
  --init \
  --memory 8g \
  --cpus 4 \
  --pids-limit 4096 \
  --network "$NET" \
  --label oo.project=open-ontology \
  --label oo.owner=a110 \
  --label oo.session="$SESSION" \
  --label oo.run_id="$RUN_ID" \
  --label oo.kind=rust \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_TERM_COLOR=never \
  -v "$EV/snapshot":/src \
  -v "$EV/raw/phases":/phases \
  -v "$EV/run/inside.sh":/inside.sh:ro \
  -v "$EV/probe":/probe:ro \
  -v "$ROOT/secrets":/fixture:ro \
  -v "$CARGO_VOL":/cargo \
  -v "$TARGET_VOL":/target \
  -w /src \
  "$RUST_IMAGE" \
  bash /inside.sh > "$EV/raw/admin/rust-container-id.txt"

printf '%s\n' "$NAME" > "$EV/raw/admin/rust-container-name.txt"
echo "rust_started name=$NAME"
