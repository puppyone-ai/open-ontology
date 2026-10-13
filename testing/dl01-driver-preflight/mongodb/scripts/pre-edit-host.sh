#!/bin/bash
# Host side of the pre-edit Linux compile. One Rust builder, then release the slot.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/testing/dl01-driver-preflight/mongodb"
EVIDENCE="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-driver-preflight"
PRE="$EVIDENCE/raw/pre-edit"
PREFIX="oo-dl01w1-41"
RUST="${PREFIX}-rust-pre"
RUST_IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
ADMIT="$SRC/scripts/admit.py"
LABELS=(
  --label project=open-ontology
  --label owner=dl01-w1-41
  --label session=23df6145-4592-4dc0-82df-fd226dd020fd
  --label task=DL01-W1-41
  --label kind=rust-builder
)

mkdir -p "$PRE"
docker image inspect "$RUST_IMAGE" >/dev/null
if ! docker network inspect "${PREFIX}-net" >/dev/null 2>&1; then
  docker network create \
    --label project=open-ontology --label owner=dl01-w1-41 \
    --label session=23df6145-4592-4dc0-82df-fd226dd020fd \
    --label task=DL01-W1-41 --label kind=mongodb-driver-preflight \
    "${PREFIX}-net" >/dev/null
fi
if ! docker volume inspect "${PREFIX}-cargo" >/dev/null 2>&1; then
  docker volume create "${PREFIX}-cargo" \
    --label project=open-ontology --label owner=dl01-w1-41 \
    --label session=23df6145-4592-4dc0-82df-fd226dd020fd \
    --label task=DL01-W1-41 --label kind=rust-builder >/dev/null
fi
if ! docker volume inspect "${PREFIX}-target" >/dev/null 2>&1; then
  docker volume create "${PREFIX}-target" \
    --label project=open-ontology --label owner=dl01-w1-41 \
    --label session=23df6145-4592-4dc0-82df-fd226dd020fd \
    --label task=DL01-W1-41 --label kind=rust-builder >/dev/null
fi

python3 "$ADMIT" acquire rust "$PRE/admission.json"
cleanup() {
  docker rm -f "$RUST" >/dev/null 2>&1 || true
  python3 "$ADMIT" release rust >/dev/null 2>&1 || true
}
trap cleanup EXIT

set +e
docker run --name "$RUST" \
  --platform linux/arm64 --pull never \
  --network "${PREFIX}-net" \
  --memory 6g --memory-swap 6g --cpus 1 \
  --pids-limit 1024 \
  -e CARGO_BUILD_JOBS=1 \
  -e CARGO_HOME=/cargo \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_INCREMENTAL=1 \
  -v "${PREFIX}-cargo:/cargo" \
  -v "${PREFIX}-target:/target" \
  -v "$SRC:/src" \
  -v "$PRE:/evidence" \
  "${LABELS[@]}" \
  "$RUST_IMAGE" \
  bash /src/scripts/pre-edit-inside.sh >"$PRE/docker.stdout" 2>"$PRE/docker.stderr"
rc=$?
set -e
printf '%s\n' "$rc" >"$PRE/docker.exit"
exit "$rc"
