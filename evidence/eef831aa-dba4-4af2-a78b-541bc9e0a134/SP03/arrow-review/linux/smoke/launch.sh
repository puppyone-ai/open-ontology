#!/bin/bash
# One owned linux/arm64 Rust 1.94 smoke builder for fixed commit 75b993d0.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a134/SP03/arrow-review/linux/smoke
ROOT=/tmp/oo-a134-eef831aa
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a134-msrv-smoke
CARGO_VOL=oo-a134-eef831aa-cargo
TARGET_VOL=oo-a134-eef831aa-target
FIXED=75b993d0b578fa863a32c64abc0073ed92d51a5d
SNAP=${ROOT}/snapshot

mkdir -p "$EVIDENCE" "$ROOT"
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"

docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}' > "$EVIDENCE/containers-before.txt"
ps -axo pid=,ppid=,etime=,comm= > "$EVIDENCE/processes-comm-only.txt"
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a134 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ]; then
  echo "deferred: this task already has a running rust builder" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if [ "$((running_rust + host_cargo))" -ge 2 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi

rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive "$FIXED" | tar -x -C "$SNAP"
printf '%s\n' "$FIXED" > "$EVIDENCE/archive-base.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/archive-tree.txt"
test -f "$SNAP/contracts/source/pgsql/empty-catalog.json"
test -f "$SNAP/crates/oo-connectors/src/stream.rs"
test ! -f "$SNAP/crates/oo-connectors/tests/pg_stream_review.rs"

BUF=crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs
committed=$(git -C "$SRC" rev-parse "${FIXED}:${BUF}")
archived=$(git -C "$SRC" hash-object "$SNAP/$BUF")
ccd=$(git -C "$SRC" rev-parse "ccd93e1adc34b24ab998d90972549f8cffb59f0d:${BUF}")
worktree=$(git -C "$SRC" hash-object "$SRC/$BUF")
printf 'fixed_commit_blob=%s\narchived_blob=%s\nccd93_author_base_blob=%s\nworktree_a133_blob=%s\n' \
  "$committed" "$archived" "$ccd" "$worktree" > "$EVIDENCE/buffered-blob.txt"
shasum -a 256 "$SNAP/$BUF" | awk '{print $1}' > "$EVIDENCE/expect-buffered.sha256"
git -C "$SRC" show "ccd93e1adc34b24ab998d90972549f8cffb59f0d:${BUF}" | shasum -a 256 | awk '{print $1}' > "$EVIDENCE/forbid-ccd93-buffered.sha256"
shasum -a 256 "$SRC/$BUF" | awk '{print $1}' > "$EVIDENCE/forbid-worktree-buffered.sha256"
test "$committed" = "$archived"
test "$archived" != "$ccd"
test "$archived" != "$worktree"
shasum -a 256 "$SNAP/contracts/source/pgsql/empty-catalog.json" > "$EVIDENCE/empty-catalog.host.sha256"
git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/src/stream.rs" > "$EVIDENCE/stream-blob.txt"
git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/src/types/mod.rs" > "$EVIDENCE/types-mod-blob.txt"
git -C "$SRC" show "$FIXED" -- crates/oo-connectors/src/types/mod.rs > "$EVIDENCE/types-mod.diff"

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology --label oo.owner=a134 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a134 --label oo.kind=cargo \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology --label oo.owner=a134 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a134 --label oo.kind=target \
  "$TARGET_VOL" >/dev/null

if docker container inspect "$NAME" >/dev/null 2>&1; then
  prior=$(docker inspect -f '{{.State.Status}}' "$NAME")
  printf 'prior_status=%s\n' "$prior" > "$EVIDENCE/prior-container.txt"
  if [ "$prior" = "running" ]; then
    echo "deferred: named container is already running" >&2
    printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
    exit 3
  fi
  docker rm "$NAME" >> "$EVIDENCE/prior-container.txt"
fi

cat > "$EVIDENCE/invocation.txt" <<EOF
source_commit=${FIXED}
tree=$(cat "$EVIDENCE/archive-tree.txt")
composition=fixed commit tree, includes panic correction 79515fcf, excludes a133 worktree vendor
author_build=ccd93 archive plus overlay, not this smoke
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
container=${NAME}
pull=never
privileged=no
home_mount=no
socket_mount=no
host_network=no
shared_cargo_cache=no
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
  --network bridge \
  --workdir /src \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/tmp \
  --mount "type=bind,source=${SNAP},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a134 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a134 \
  --label oo.task=arrow-review-smoke \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
  echo "Status=$(docker inspect -f '{{.State.Status}}' "$NAME")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{.HostConfig.CapDrop}}' "$NAME")"
} > "$EVIDENCE/hostconfig.txt"
exit "$run_exit"
