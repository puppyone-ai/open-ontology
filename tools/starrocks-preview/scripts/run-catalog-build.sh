#!/bin/bash
# Archive a6825165 and compile it once under the W45 rust slot.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/tools/starrocks-preview"
EVIDENCE="$ROOT/evidence/2c023948-0862-4dae-ae8f-6fc30241db62/DL01/starrocks-product/raw/native-catalog-build"
ARCHIVE="/tmp/oo-dl01w1-45-catalog-src"
BINS="/tmp/oo-dl01w1-45-catalog-bins"
PREFIX="oo-dl01w1-45"
RUST="${PREFIX}-rust-catalog"
RUST_IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
ADMIT="$ROOT/testing/dl01-driver-preflight/starrocks/scripts/admit.py"
PIN="a6825165dd83ffaa30e17fbb2da1a481a05f6559"
STATUS="/tmp/oo-dl01w1-45-catalog.status"

phase() {
  mkdir -p "$EVIDENCE"
  printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" | tee -a "$EVIDENCE/host-phase.txt"
}

prepare_archive() {
  rm -rf "$ARCHIVE"
  mkdir -p "$ARCHIVE" "$BINS" "$EVIDENCE"
  git -C "$ROOT" archive "$PIN" \
    Cargo.toml Cargo.lock \
    crates \
    tools/pg-builder tools/description-pack tools/description-review \
    tools/operator-preview tools/gateway-audit-wal \
    tools/gateway-token-store tools/minimal-product-loop tools/pi-harness \
    testing/real-db/pg-e2e \
    | tar -C "$ARCHIVE" -xf -
  if [[ -e "$ARCHIVE/rust-toolchain.toml" ]]; then
    phase "FAIL archive contains a toolchain file"
    exit 1
  fi
  git -C "$ROOT" rev-parse "$PIN" | tee "$EVIDENCE/source-pin.txt"
  printf '%s\n' "Private archive of ${PIN}. Live workspace sources were not mounted." > "$EVIDENCE/archive-note.txt"
}

release_rust() {
  docker rm -f "$RUST" >/dev/null 2>&1 || true
  python3 "$ADMIT" release rust >/dev/null 2>&1 || true
}

run_once() {
  set -euo pipefail
  docker image inspect "$RUST_IMAGE" >/dev/null
  prepare_archive
  if docker ps -a --format '{{.Names}}' | grep -qx "$RUST"; then
    docker rm -f "$RUST" >/dev/null
  fi
  set +e
  python3 "$ADMIT" acquire-start rust "$EVIDENCE/capacity.json" -- \
    docker run -d --name "$RUST" \
      --platform linux/arm64 --pull never \
      --network oo-dl01w1-45 \
      --memory 6g --memory-swap 6g --cpus 1 \
      --pids-limit 1024 \
      -e CARGO_BUILD_JOBS=1 \
      -e CARGO_HOME=/cargo \
      -e CARGO_TERM_COLOR=never \
      -e RUSTUP_TOOLCHAIN=1.94.0 \
      -e HOST_UID="$(id -u)" \
      -e HOST_GID="$(id -g)" \
      -v "${PREFIX}-cargo:/cargo" \
      -v "${PREFIX}-target:/target" \
      -v "$ARCHIVE:/src:ro" \
      -v "$EVIDENCE:/evidence" \
      -v "$BINS:/bins" \
      -v "$SRC/scripts/build-catalog-inside.sh:/opt/build-catalog-inside.sh:ro" \
      --label project=open-ontology \
      --label owner=2c023948-0862-4dae-ae8f-6fc30241db62 \
      --label session=2c023948-0862-4dae-ae8f-6fc30241db62 \
      --label task=DL01-W1-45 \
      --label kind=rust-builder \
      "$RUST_IMAGE" \
      bash /opt/build-catalog-inside.sh
  start_rc=$?
  set -e
  if [[ "$start_rc" -eq 2 ]]; then
    phase "catalog capacity denied"
    return 3
  fi
  if [[ "$start_rc" -ne 0 ]]; then
    phase "catalog docker start failed ${start_rc}"
    return "$start_rc"
  fi
  phase "catalog container started"
  set +e
  docker wait "$RUST" >"$EVIDENCE/docker.exit"
  wait_rc=$?
  docker logs "$RUST" >"$EVIDENCE/docker.stdout" 2>"$EVIDENCE/docker.stderr"
  set -e
  build_rc="$(cat "$EVIDENCE/docker.exit")"
  release_rust
  phase "catalog docker_exit=${build_rc} wait=${wait_rc}"
  return "$build_rc"
}

rm -f "$STATUS"
phase "catalog build wait start"
for attempt in $(seq 1 30); do
  phase "catalog capacity attempt ${attempt}"
  set +e
  ( run_once )
  rc=$?
  set -e
  if [[ "$rc" -eq 0 ]]; then
    echo "DONE catalog-build ${rc}" > "$STATUS"
    exit 0
  fi
  if [[ "$rc" -ne 3 ]]; then
    echo "FAILED catalog-build ${rc}" > "$STATUS"
    exit "$rc"
  fi
  phase "catalog sleep 900"
  sleep 900
done
echo "FAILED catalog-build capacity" > "$STATUS"
exit 3
