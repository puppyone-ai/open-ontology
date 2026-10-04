#!/bin/bash
# One owned linux/arm64 Rust 1.94 focused check. Immutable archive plus overlay.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a156/SP07/captured-pg-draft
ROOT=/tmp/oo-a156-eef831aa
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a156-builder-focused
CARGO_VOL=oo-a156-eef831aa-cargo
TARGET_VOL=oo-a156-eef831aa-target
NET=oo-a156-eef831aa-net
FIXED=e6bd4c7aa8c3a43cc0516a21e325765c5eecb2f6
SNAP=${ROOT}/focused-snapshot

mkdir -p "$EVIDENCE/linux/focused" "$ROOT"
chmod 755 "$EVIDENCE/linux/focused/inside.sh" "$EVIDENCE/linux/focused/launch.sh"

docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}' > "$EVIDENCE/linux/focused/containers-before.txt" || true
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a156 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$EVIDENCE/linux/focused/capacity.txt"
if [ "$own" -ge 1 ]; then
  echo "deferred: this task already has a running rust builder" >&2
  printf '%s\n' 3 > "$EVIDENCE/linux/focused/docker-run.exit"
  exit 3
fi
if [ "$((running_rust + host_cargo))" -ge 2 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$EVIDENCE/linux/focused/docker-run.exit"
  exit 3
fi

rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml crates .cargo examples/ontology-draft \
  | tar -x -C "$SNAP"
overlay() {
  cp "$SRC/$1" "$SNAP/$1"
}
overlay crates/oo-builder/src/lib.rs
overlay crates/oo-builder/src/draft.rs
overlay crates/oo-builder/src/pgsql.rs
overlay crates/oo-builder/src/captured_pg.rs
overlay crates/oo-builder/tests/captured_pg.rs
mkdir -p "$SNAP/contracts/builder"
cp "$SRC/contracts/builder/captured-pg-draft-v1.md" "$SNAP/contracts/builder/captured-pg-draft-v1.md"
test -f "$SNAP/crates/oo-builder/src/captured_pg.rs"
test -f "$SNAP/examples/ontology-draft/MANIFEST.sha256"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/linux/focused/source-tree.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/linux/focused/source-commit.txt"

if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a156 \
    --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a156 \
    "$NET" > "$EVIDENCE/linux/focused/network.txt"
fi
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology --label oo.owner=a156 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a156 --label oo.kind=cargo \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology --label oo.owner=a156 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a156 --label oo.kind=target \
  "$TARGET_VOL" >/dev/null

if docker container inspect "$NAME" >/dev/null 2>&1; then
  prior=$(docker inspect -f '{{.State.Status}}' "$NAME")
  printf 'prior_status=%s\n' "$prior" > "$EVIDENCE/linux/focused/prior-container.txt"
  if [ "$prior" = "running" ]; then
    echo "deferred: named container is already running" >&2
    printf '%s\n' 3 > "$EVIDENCE/linux/focused/docker-run.exit"
    exit 3
  fi
  docker rm "$NAME" >> "$EVIDENCE/linux/focused/prior-container.txt"
fi

cat > "$EVIDENCE/linux/focused/invocation.txt" <<EOF
source_commit=${FIXED}
tree=$(cat "$EVIDENCE/linux/focused/source-tree.txt")
snapshot_method=git archive of ${FIXED} plus overlay of captured-pg sources
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
container=${NAME}
network=${NET}
pull=never
privileged=no
EOF

set +e
docker run \
  --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 8g \
  --memory-swap 8g \
  --cpus 4 \
  --pids-limit 4096 \
  --network "$NET" \
  --workdir /src \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/tmp \
  --mount "type=bind,source=${SNAP},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/linux/focused/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a156 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a156 \
  --label oo.task=captured-pg-draft-focused \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/linux/focused/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/linux/focused/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
  echo "Status=$(docker inspect -f '{{.State.Status}}' "$NAME")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
  echo "CapAdd=$(docker inspect -f '{{json .HostConfig.CapAdd}}' "$NAME")"
  echo "PortBindings=$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$NAME")"
} > "$EVIDENCE/linux/focused/hostconfig.txt"
exit "$run_exit"
