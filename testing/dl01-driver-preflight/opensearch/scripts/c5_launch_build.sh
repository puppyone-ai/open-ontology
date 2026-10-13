#!/bin/bash
# Linux Rust 1.94 test, clippy, and build for the C5 reader.
# Does not start oo-dl01w1-44-rust and does not delete volumes.
set -euo pipefail
ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/testing/dl01-driver-preflight/opensearch"
EVIDENCE="$ROOT/evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-driver-preflight/c5-loop-20261012"
BASE="/tmp/oo-dl01w1-44-base/Cargo.lock"
NAME="oo-dl01w1-44-c5-rust"
IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
mkdir -p "$EVIDENCE/build"
if [ ! -f "$BASE" ]; then
  echo "base lock missing" >&2
  exit 2
fi
if docker ps -a --format '{{.Names}}' | grep -qx "$NAME"; then
  owner="$(docker inspect -f '{{index .Config.Labels "owner"}}' "$NAME")"
  status="$(docker inspect -f '{{.State.Status}}' "$NAME")"
  if [ "$owner" != "oo-dl01w1-44" ]; then
    echo "refusing to remove foreign container" >&2
    exit 1
  fi
  if [ "$status" = "running" ]; then
    echo "c5 rust container is already running" >&2
    exit 1
  fi
  docker rm "$NAME" >/dev/null
fi
python3 "$SRC/scripts/admit.py" acquire rust "$EVIDENCE/build/admission.json"
if [ ! -f "$EVIDENCE/build/admission.json" ]; then
  echo "admission file missing" >&2
  exit 3
fi
python3 - << 'PY'
import json, sys
body = json.load(open("/Users/redux/puppy-one/open-ontology/evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-driver-preflight/c5-loop-20261012/build/admission.json"))
if not body.get("admitted"):
    print(body.get("reason") or "not admitted")
    sys.exit(3)
print("admitted", body.get("global_rust_count"), body.get("headroom_after_reservation_bytes"))
PY
if ! docker run -d --name "$NAME" --platform linux/arm64 --pull never --init --user 0:0 \
  --security-opt no-new-privileges --cap-drop ALL --memory 6g --memory-swap 6g --cpus 1 --pids-limit 1024 \
  -e CARGO_BUILD_JOBS=1 -e RUSTUP_TOOLCHAIN=1.94.0 \
  -v "$SRC:/src" \
  -v "$EVIDENCE:/evidence" \
  -v "$BASE:/base/Cargo.lock:ro" \
  -v "$SRC/scripts/c5_build_inside.sh:/inside.sh:ro" \
  -v oo-dl01w1-44-cargo-reg:/usr/local/cargo/registry \
  -v oo-dl01w1-44-cargo-git:/usr/local/cargo/git \
  -v oo-dl01w1-44-target:/target \
  --label project=open-ontology \
  --label owner=oo-dl01w1-44 \
  --label session=337bb7c5-aaf7-4001-b4fe-e70de40ac91b \
  --label kind=rust-builder \
  --label task=DL01-W1-44 \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  "$IMAGE" \
  bash /inside.sh; then
  python3 "$SRC/scripts/admit.py" release rust >/dev/null || true
  echo "docker run failed after acquire; rust slot released" >&2
  exit 1
fi
set +e
status="$(docker wait "$NAME")"
wait_code=$?
set -e
python3 "$SRC/scripts/admit.py" release rust >/dev/null || true
if [ "$wait_code" != "0" ]; then
  exit "$wait_code"
fi
exit "$status"
