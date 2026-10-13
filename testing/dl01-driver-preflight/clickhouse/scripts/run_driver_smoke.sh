#!/bin/bash
# Linux Rust 1.94 driver smoke. Keeps cargo and target volumes. Does not write the historical build/ tree.
set -u
ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/testing/dl01-driver-preflight/clickhouse"
EVIDENCE="${OO_SMOKE_EVIDENCE:-$ROOT/evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-driver-preflight/smoke-20261012}"
ADMIT="$SRC/scripts/admit.py"
BASE="/tmp/oo-dl01w1-40-base-577160c.lock"
LOG="$EVIDENCE/smoke.log"
mkdir -p "$EVIDENCE"
exec >>"$LOG" 2>&1
echo "SMOKE_START $(date -u +%Y-%m-%dT%H:%M:%SZ)"
fail() {
  printf '%s\n' "$1" > "$EVIDENCE/SMOKE_FAIL"
  echo "SMOKE_FAIL $1"
  exit 1
}
if docker ps --format '{{.Names}}' | grep -qx 'oo-dl01w1-40-rust'; then
  fail "rust container already running"
fi
if docker ps -a --format '{{.Names}}' | grep -qx 'oo-dl01w1-40-rust'; then
  owner="$(docker inspect -f '{{index .Config.Labels "oo.owner"}}' oo-dl01w1-40-rust)"
  status="$(docker inspect -f '{{.State.Status}}' oo-dl01w1-40-rust)"
  if [ "$owner" != "oo-dl01w1-40" ]; then
    fail "foreign rust container"
  fi
  if [ "$status" = "running" ]; then
    fail "rust container is already running"
  fi
  docker rm oo-dl01w1-40-rust >/dev/null || fail "could not remove stopped rust container"
fi
git -C "$ROOT" show 577160c22de63adb9b38e0147f3bc2da893c4d9d:Cargo.lock > "$BASE" || fail "base lock missing"
docker network inspect oo-dl01w1-40-net >/dev/null 2>&1 || docker network create --subnet 172.16.40.0/24 --gateway 172.16.40.1 oo-dl01w1-40-net >/dev/null || fail "network"
start_rust() {
  python3 "$ADMIT" acquire-start rust "$EVIDENCE/admission.json" -- \
    docker run -d --name oo-dl01w1-40-rust --platform linux/arm64 --pull never --init --user 0:0 \
    --security-opt no-new-privileges --cap-drop ALL --memory 6g --memory-swap 6g --cpus 1 --pids-limit 1024 \
    --network oo-dl01w1-40-net \
    -e HOME=/tmp -e CARGO_HOME=/cargo-home -e CARGO_TARGET_DIR=/target/driver -e CARGO_BUILD_JOBS=1 -e RUSTUP_TOOLCHAIN=1.94.0 \
    -v "$SRC:/src" \
    -v "$EVIDENCE:/evidence" \
    -v "$SRC/scripts/build_inside.sh:/inside.sh:ro" \
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
    bash /inside.sh
}
if ! start_rust; then
  reason="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("reason",""))' "$EVIDENCE/admission.json" 2>/dev/null || true)"
  echo "REFUSED $reason"
  echo "WAIT_900 $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  python3 -c 'import time; time.sleep(900)'
  start_rust || fail "admission refused after one wait"
fi
echo "RUST_STARTED $(date -u +%Y-%m-%dT%H:%M:%SZ)"
ready=0
for _ in $(seq 1 180); do
  if [ -f "$EVIDENCE/build/lock-ready" ]; then
    ready=1
    break
  fi
  if ! docker ps --format '{{.Names}}' | grep -qx 'oo-dl01w1-40-rust'; then
    fail "rust exited before lock-ready"
  fi
  sleep 1
done
if [ "$ready" != "1" ]; then
  fail "lock-ready timeout"
fi
if python3 "$SRC/scripts/check_lock.py" "$EVIDENCE/build/Cargo.lock" "$BASE" "$EVIDENCE/build/lock-check.json"; then
  : > "$EVIDENCE/build/lock-accepted"
else
  : > "$EVIDENCE/build/lock-rejected"
  fail "lock rejected"
fi
for _ in $(seq 1 720); do
  if [ -f "$EVIDENCE/build/build.exit" ]; then
    break
  fi
  if ! docker ps --format '{{.Names}}' | grep -qx 'oo-dl01w1-40-rust'; then
    fail "rust exited before build.exit"
  fi
  sleep 10
done
if [ ! -f "$EVIDENCE/build/build.exit" ]; then
  fail "build.exit timeout"
fi
code="$(tr -d '[:space:]' < "$EVIDENCE/build/build.exit")"
docker stop oo-dl01w1-40-rust >/dev/null 2>&1 || true
python3 "$ADMIT" release rust >/dev/null 2>&1 || true
printf '%s\n' "$code" > "$EVIDENCE/smoke.exit"
if [ "$code" != "0" ]; then
  fail "build.exit $code"
fi
echo "SMOKE_DONE $(date -u +%Y-%m-%dT%H:%M:%SZ)"
