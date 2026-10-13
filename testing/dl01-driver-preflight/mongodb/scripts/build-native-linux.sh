#!/bin/bash
# Linux 1.94 build of the native capture producer. No CPU quota and no fixed job cap.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
EVIDENCE="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/native-linux-194"
IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
NAME="oo-dl01w1-41-rust-native"
VOLUME="oo-dl01w1-41-native-target"

if [[ -e "$EVIDENCE" ]]; then
  echo "evidence already exists" >&2
  exit 2
fi
if docker ps -a --format '{{.Names}}' | grep -qx "$NAME"; then
  echo "named builder already present" >&2
  exit 2
fi
if docker volume ls -q | grep -qx "$VOLUME"; then
  echo "named target volume already present" >&2
  exit 2
fi
mkdir -p "$EVIDENCE"
git -C "$ROOT" status --short -- tools/source-admission crates/oo-connectors/vendor/sqlx-core crates/oo-connectors/vendor/sqlx-postgres > "$EVIDENCE/foreign-status.txt"
git -C "$ROOT" rev-parse HEAD > "$EVIDENCE/git-head.txt"
git -C "$ROOT" rev-parse HEAD:crates/oo-core/src/jsonb_doc.rs > "$EVIDENCE/jsonb-doc.blob"
git -C "$ROOT" rev-parse 983577c9b89453b08ab4e1a02cab88df78c3e005:crates/oo-core/src/jsonb_doc.rs > "$EVIDENCE/jsonb-doc.fixed"
docker image inspect "$IMAGE" --format '{{.Id}} {{.Os}}/{{.Architecture}}' > "$EVIDENCE/image.txt"
docker volume create "$VOLUME" >/dev/null
cleanup() {
  docker rm -f "$NAME" >/dev/null 2>&1 || true
}
trap cleanup EXIT
set +e
docker run --name "$NAME" \
  --platform linux/arm64 --pull never \
  --memory 24g --memory-swap 24g \
  --pids-limit 4096 \
  -e HOST_UID="$(id -u)" \
  -e HOST_GID="$(id -g)" \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -v "$ROOT:/work:ro" \
  -v "$VOLUME:/target" \
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
docker inspect "$NAME" --format 'ExitCode={{.State.ExitCode}} OOMKilled={{.State.OOMKilled}} Error={{.State.Error}} NanoCpus={{.HostConfig.NanoCpus}} Memory={{.HostConfig.Memory}}' > "$EVIDENCE/inspect.txt" || true
exit "$status"
