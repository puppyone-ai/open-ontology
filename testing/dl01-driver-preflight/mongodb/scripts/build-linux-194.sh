#!/bin/bash
# One Linux 1.94 container for the Mongo preview. No CPU quota and no fixed job cap.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
EVIDENCE="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/c5-linux-194-presence"
IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
NAME="oo-dl01w1-41-rust-c5"

if [[ -e "$EVIDENCE" ]]; then
  echo "evidence already exists" >&2
  exit 2
fi
if docker ps -a --format '{{.Names}}' | grep -qx "$NAME"; then
  echo "named builder already present" >&2
  exit 2
fi
mkdir -p "$EVIDENCE"
git -C "$ROOT" status --short -- tools/source-admission crates/oo-connectors/vendor/sqlx-core crates/oo-connectors/vendor/sqlx-postgres > "$EVIDENCE/foreign-status.txt"
docker image inspect "$IMAGE" >/dev/null
docker volume create oo-dl01w1-41-c5target194 >/dev/null
cleanup() {
  docker rm -f "$NAME" >/dev/null 2>&1 || true
}
trap cleanup EXIT
set +e
docker run --name "$NAME" \
  --platform linux/arm64 --pull never \
  --memory 16g --memory-swap 16g \
  --pids-limit 4096 \
  -e HOST_UID="$(id -u)" \
  -e HOST_GID="$(id -g)" \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -v "$ROOT:/work:ro" \
  -v "oo-dl01w1-41-c5target194:/target" \
  -v "$EVIDENCE:/evidence" \
  -v "$ROOT/tools/mongodb-preview/scripts/build-c5-inside.sh:/build-c5-inside.sh:ro" \
  --label project=open-ontology \
  --label owner=dl01-w1-41 \
  --label task=DL01-W1-41 \
  --label kind=rust-builder \
  "$IMAGE" \
  bash /build-c5-inside.sh
status=$?
set -e
printf '%s\n' "$status" > "$EVIDENCE/docker.exit"
exit "$status"
