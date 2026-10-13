#!/bin/bash
# One Rust slot. Builds oo-pipeline, pg-builder, and source-semantics from the
# extracted 631720c5f archive. Does not mount the live tree and does not start
# Mongo or Postgres.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/testing/dl01-driver-preflight/mongodb"
ARCHIVE="/tmp/oo-w141-archive"
EVIDENCE="${FIXED_BUILD_EVIDENCE:-$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-65-fixed-build}"
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

if [[ -e "$EVIDENCE" ]]; then
  echo "evidence dir already exists" >&2
  exit 2
fi
if docker ps -a --format '{{.Names}}' | grep -qx "$RUST"; then
  echo "named container $RUST still present" >&2
  exit 2
fi
if [[ ! -f "$ARCHIVE/crates/oo-pipeline/src/bin/oo-pipeline.rs" ]]; then
  echo "fixed archive is not extracted" >&2
  exit 2
fi

mkdir -p "$EVIDENCE"
printf '%s\n' "631720c5f0e9eaf07fa1616382003664e765595d" > "$EVIDENCE/source-commit.txt"
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
  -e CARGO_INCREMENTAL=1 \
  -e HOST_UID="$(id -u)" \
  -e HOST_GID="$(id -g)" \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -v "${PREFIX}-cargo:/cargo" \
  -v "${PREFIX}-target:/target" \
  -v "$ARCHIVE:/src:ro" \
  -v "$SRC/scripts/build-fixed-inside.sh:/inside.sh:ro" \
  -v "$EVIDENCE:/evidence" \
  "${LABELS[@]}" \
  "$RUST_IMAGE" \
  bash /inside.sh >"$EVIDENCE/docker.stdout" 2>"$EVIDENCE/docker.stderr"
rc=$?
set -e
printf '%s\n' "$rc" >"$EVIDENCE/docker.exit"
if [[ "$rc" -ne 0 && ! -f "$EVIDENCE/status.txt" ]]; then
  printf '%s\n' "FAILED docker $rc" >"$EVIDENCE/status.txt"
fi
exit "$rc"
