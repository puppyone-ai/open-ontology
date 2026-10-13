#!/bin/bash
# Remove the finished pre-edit container, admit one Rust slot, start the build.
set -euo pipefail
ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/testing/dl01-driver-preflight/opensearch"
PREVIEW="$ROOT/tools/opensearch-preview"
EVIDENCE="$ROOT/evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-driver-preflight"
BASE="/tmp/oo-dl01w1-44-base/Cargo.lock"
ARCH="/tmp/oo-dl01w1-44-archives/5df40d24eefa4fbcfa00c4415cfcfde5d368db89"
mkdir -p "$EVIDENCE/build"
if [ ! -f "$ARCH/tools/source-admission/Cargo.toml" ] \
  || [ ! -f "$ARCH/crates/oo-connectors/vendor/sqlx-core/Cargo.toml" ] \
  || [ ! -f "$ARCH/crates/oo-connectors/vendor/sqlx-postgres/Cargo.toml" ]; then
  echo "pinned git archive missing under $ARCH" >&2
  exit 2
fi
if docker ps -a --format '{{.Names}}' | grep -qx 'oo-dl01w1-44-rust'; then
  owner="$(docker inspect -f '{{index .Config.Labels "owner"}}' oo-dl01w1-44-rust)"
  status="$(docker inspect -f '{{.State.Status}}' oo-dl01w1-44-rust)"
  if [ "$owner" != "oo-dl01w1-44" ]; then
    echo "refusing to remove foreign container" >&2
    exit 1
  fi
  if [ "$status" = "running" ]; then
    echo "rust container is already running" >&2
    exit 1
  fi
  docker rm oo-dl01w1-44-rust >/dev/null
fi
python3 "$SRC/scripts/admit.py" release rust >/dev/null
python3 "$SRC/scripts/admit.py" acquire rust "$EVIDENCE/build/admission.json"
if ! docker run -d --name oo-dl01w1-44-rust --platform linux/arm64 --pull never --init --user 0:0 \
  --security-opt no-new-privileges --cap-drop ALL --memory 6g --memory-swap 6g --cpus 1 --pids-limit 1024 \
  -e CARGO_BUILD_JOBS=1 -e RUSTUP_TOOLCHAIN=1.94.0 \
  -v "$SRC:/src" \
  -v "$PREVIEW:/preview" \
  -v "$ARCH/tools/source-admission:/source-admission:ro" \
  -v "$ARCH/crates/oo-connectors/vendor/sqlx-core:/crates/oo-connectors/vendor/sqlx-core:ro" \
  -v "$ARCH/crates/oo-connectors/vendor/sqlx-postgres:/crates/oo-connectors/vendor/sqlx-postgres:ro" \
  -v "$EVIDENCE:/evidence" \
  -v "$BASE:/base/Cargo.lock:ro" \
  -v "$SRC/scripts/build_inside.sh:/inside.sh:ro" \
  -v oo-dl01w1-44-cargo-reg:/usr/local/cargo/registry \
  -v oo-dl01w1-44-cargo-git:/usr/local/cargo/git \
  -v oo-dl01w1-44-target:/target \
  --label project=open-ontology \
  --label owner=oo-dl01w1-44 \
  --label session=bc6cf794-2d14-4e98-bcb6-f7a9b3533a88 \
  --label kind=rust-builder \
  --label task=DL01-W1-44 \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 \
  bash /inside.sh; then
  python3 "$SRC/scripts/admit.py" release rust >/dev/null || true
  echo "docker run failed after acquire; rust slot released" >&2
  exit 1
fi
