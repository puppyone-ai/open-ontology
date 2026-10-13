#!/bin/bash
# Product test compile on Linux Rust 1.94. One owned builder. Does not remove the cargo volume.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/tools/mongodb-preview"
EVIDENCE="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product"
OUT="$EVIDENCE/raw/product"
PREFIX="oo-dl01w1-41"
RUST="${PREFIX}-rust"
RUST_IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
ADMIT="$ROOT/testing/dl01-driver-preflight/mongodb/scripts/admit.py"
LABELS=(
  --label project=open-ontology
  --label owner=dl01-w1-41
  --label session=23df6145-4592-4dc0-82df-fd226dd020fd
  --label task=DL01-W1-41
)
final_ok=0

phase() {
  mkdir -p "$EVIDENCE/raw"
  printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" | tee -a "$EVIDENCE/raw/phase.txt"
}

remove_owned() {
  local id owner
  for id in $(docker ps -aq --filter "name=${PREFIX}"); do
    owner="$(docker inspect -f '{{index .Config.Labels "owner"}}' "$id" 2>/dev/null || true)"
    if [[ "$owner" == "dl01-w1-41" ]]; then
      docker rm -f "$id" >/dev/null
    fi
  done
}

admit_wait() {
  local kind="$1" dest="$2" attempt
  mkdir -p "$(dirname "$dest")"
  for attempt in $(seq 1 30); do
    if python3 "$ADMIT" acquire "$kind" "$dest"; then
      return 0
    fi
    phase "capacity wait ${kind} attempt ${attempt}"
    sleep 900
  done
  phase "capacity denied ${kind}"
  return 3
}

cleanup() {
  local rc=$?
  if [[ "$rc" -eq 0 ]]; then
    final_ok=1
  fi
  docker rm -f "$RUST" >/dev/null 2>&1 || true
  python3 "$ADMIT" release rust >/dev/null 2>&1 || true
  phase "product cleanup final_ok=${final_ok}"
  exit "$rc"
}
trap cleanup EXIT

phase "product start"
docker image inspect "$RUST_IMAGE" >/dev/null
mkdir -p "$OUT"
remove_owned
admit_wait rust "$OUT/admission.json"
phase "product admitted"
set +e
docker run --name "$RUST" \
  --platform linux/arm64 --pull never \
  --memory 6g --memory-swap 6g --cpus 1 \
  --pids-limit 1024 \
  -e CARGO_BUILD_JOBS=1 \
  -e CARGO_HOME=/cargo \
  -e CARGO_TERM_COLOR=never \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TARGET_DIR=/target/mongodb-preview-194 \
  -e CARGO_INCREMENTAL=1 \
  -e HOST_UID="$(id -u)" \
  -e HOST_GID="$(id -g)" \
  -v "${PREFIX}-cargo:/cargo" \
  -v "${PREFIX}-target:/target" \
  -v "$SRC:/work/tools/mongodb-preview" \
  -v "$ROOT/tools/source-admission:/work/tools/source-admission:ro" \
  -v "$ROOT/crates/oo-connectors/vendor:/work/crates/oo-connectors/vendor:ro" \
  -v "$OUT:/evidence" \
  -w /work/tools/mongodb-preview \
  "${LABELS[@]}" \
  --label kind=rust-builder \
  "$RUST_IMAGE" \
  bash /work/tools/mongodb-preview/scripts/build-product-inside.sh >"$OUT/docker.stdout" 2>"$OUT/docker.stderr"
build_rc=$?
set -e
printf '%s\n' "$build_rc" >"$OUT/docker.exit"
docker rm -f "$RUST" >/dev/null 2>&1 || true
python3 "$ADMIT" release rust >/dev/null
if [[ "$build_rc" -ne 0 ]]; then
  phase "product build failed ${build_rc}"
  exit "$build_rc"
fi
phase "product build ok"
final_ok=1
