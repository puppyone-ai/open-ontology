#!/bin/bash
# One Rust 1.94 slot for the W41 repair. Does not start Mongo or Postgres.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/testing/dl01-driver-preflight/mongodb"
PREVIEW="$ROOT/tools/mongodb-preview"
EVIDENCE="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-driver-preflight/raw/repair-59"
PREFIX="oo-dl01w1-41"
RUST="${PREFIX}-rust-repair"
RUST_IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
ADMIT="$SRC/scripts/admit.py"
LABELS=(
  --label project=open-ontology
  --label owner=dl01-w1-41
  --label session=23df6145-4592-4dc0-82df-fd226dd020fd
  --label task=DL01-W1-41
  --label kind=rust-builder
)

mkdir -p "$EVIDENCE"
docker image inspect "$RUST_IMAGE" >/dev/null
python3 "$ADMIT" acquire rust "$EVIDENCE/admission.json"
cleanup() {
  docker rm -f "$RUST" >/dev/null 2>&1 || true
  python3 "$ADMIT" release rust >/dev/null 2>&1 || true
}
trap cleanup EXIT

set +e
docker run --name "$RUST" \
  --platform linux/arm64 --pull never \
  --memory 6g --memory-swap 6g --cpus 1 \
  --pids-limit 1024 \
  -e CARGO_BUILD_JOBS=1 \
  -e CARGO_HOME=/cargo \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_INCREMENTAL=1 \
  -e HOST_UID="$(id -u)" \
  -e HOST_GID="$(id -g)" \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -v "${PREFIX}-cargo:/cargo" \
  -v "${PREFIX}-target:/target" \
  -v "$SRC:/src" \
  -v "$PREVIEW:/work/tools/mongodb-preview:ro" \
  -v "$ROOT/tools/source-admission:/work/tools/source-admission:ro" \
  -v "$ROOT/crates/oo-connectors/vendor:/work/crates/oo-connectors/vendor:ro" \
  -v "$EVIDENCE:/evidence" \
  "${LABELS[@]}" \
  "$RUST_IMAGE" \
  bash /src/scripts/repair-inside.sh >"$EVIDENCE/docker.stdout" 2>"$EVIDENCE/docker.stderr"
rc=$?
set -e
printf '%s\n' "$rc" >"$EVIDENCE/docker.exit"
exit "$rc"
