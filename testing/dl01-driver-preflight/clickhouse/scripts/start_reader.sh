#!/bin/bash
# Reattach the already built Linux 1.94 binaries. This does not compile.
set -euo pipefail
cd "$(dirname "$0")/../../../.."
ROOT="$(pwd)"
EVIDENCE="$ROOT/evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-driver-preflight"
SRC="$ROOT/testing/dl01-driver-preflight/clickhouse"
mkdir -p "$EVIDENCE/build" /tmp/oo-dl01w1-40-secrets
chmod 700 /tmp/oo-dl01w1-40-secrets
if ! docker network inspect oo-dl01w1-40-net >/dev/null 2>&1; then
  echo "missing network oo-dl01w1-40-net" >&2
  exit 1
fi
if docker ps -a --format '{{.Names}}' | grep -qx 'oo-dl01w1-40-rust'; then
  owner="$(docker inspect -f '{{index .Config.Labels "oo.owner"}}' oo-dl01w1-40-rust)"
  status="$(docker inspect -f '{{.State.Status}}' oo-dl01w1-40-rust)"
  if [ "$owner" != "oo-dl01w1-40" ]; then
    echo "refusing to remove foreign container" >&2
    exit 1
  fi
  if [ "$status" = "running" ]; then
    echo "rust container is already running" >&2
    exit 1
  fi
  docker rm oo-dl01w1-40-rust >/dev/null
fi
python3 "$SRC/scripts/admit.py" acquire-start rust "$EVIDENCE/build/reader-restart-admission.json" -- \
  docker run -d --name oo-dl01w1-40-rust --platform linux/arm64 --pull never --init --user 0:0 \
  --security-opt no-new-privileges --cap-drop ALL --memory 6g --memory-swap 6g --cpus 1 --pids-limit 1024 \
  --network oo-dl01w1-40-net \
  -e HOME=/tmp -e CARGO_HOME=/cargo-home -e CARGO_TARGET_DIR=/target/driver -e CARGO_BUILD_JOBS=1 -e RUSTUP_TOOLCHAIN=1.94.0 \
  -v "$SRC:/src:ro" \
  -v "$EVIDENCE:/evidence" \
  -v /tmp/oo-dl01w1-40-secrets:/secrets:ro \
  -v oo-dl01w1-40-cargo:/cargo-home \
  -v oo-dl01w1-40-target:/target \
  --tmpfs /tmp:rw,size=512m \
  --label oo.project=open-ontology \
  --label oo.owner=oo-dl01w1-40 \
  --label oo.session=4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de \
  --label oo.kind=rust-builder \
  --label oo.task=DL01-W1-40 \
  --label oo.toolchain=1.94.0 \
  docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 \
  sleep infinity
docker exec oo-dl01w1-40-rust sha256sum /target/driver/debug/ch-reader /target/driver/debug/ch-decode \
  > "$EVIDENCE/build/binary-recheck.sha256"
if ! cmp -s "$EVIDENCE/build/binaries.sha256" "$EVIDENCE/build/binary-recheck.sha256"; then
  echo "binary hash does not match the green build; not recompiling" >&2
  docker rm -f oo-dl01w1-40-rust >/dev/null || true
  python3 "$SRC/scripts/admit.py" release rust
  exit 3
fi
echo "binary-recheck-ok"
