#!/bin/bash
# One owned linux/arm64 Rust 1.94 smoke builder. Product archive of bd77b252.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a135/SP03/init-tail-review
ROOT=/tmp/oo-a135-eef831aa
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a135-init-tail-smoke
CARGO_VOL=oo-a135-eef831aa-cargo
TARGET_VOL=oo-a135-eef831aa-target
NET=oo-a135-eef831aa-net
FIXED=bd77b2520bcc3d63517bc49309002f25b58544b1
ARROW=75b993d0b578fa863a32c64abc0073ed92d51a5d
SNAP=${ROOT}/snapshot
FIXTURE_HASH=044022fcabb7587632133cb8f81c3665292c32acc7af3432b5bfe4d066679a3c

mkdir -p "$EVIDENCE/smoke" "$ROOT"
chmod 755 "$EVIDENCE/smoke/inside.sh" "$EVIDENCE/smoke/launch.sh"

docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}' > "$EVIDENCE/smoke/containers-before.txt" || true
ps -axo pid=,ppid=,etime=,comm= > "$EVIDENCE/smoke/processes-comm-only.txt"
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a135 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$EVIDENCE/smoke/capacity.txt"
if [ "$own" -ge 1 ]; then
  echo "deferred: this task already has a running rust builder" >&2
  printf '%s\n' 3 > "$EVIDENCE/smoke/docker-run.exit"
  exit 3
fi
if [ "$((running_rust + host_cargo))" -ge 2 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$EVIDENCE/smoke/docker-run.exit"
  exit 3
fi

rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml crates .cargo contracts/source/pgsql/empty-catalog.json \
  | tar -x -C "$SNAP"
test -f "$SNAP/contracts/source/pgsql/empty-catalog.json"
test -f "$SNAP/crates/oo-connectors/src/stream.rs"
test ! -f "$SNAP/crates/oo-connectors/tests/a135_tail_probe.rs"
got=$(shasum -a 256 "$SNAP/contracts/source/pgsql/empty-catalog.json" | awk '{print $1}')
if [ "$got" != "$FIXTURE_HASH" ]; then
  echo "fixture hash ${got} != ${FIXTURE_HASH}" >&2
  printf '%s\n' 4 > "$EVIDENCE/smoke/docker-run.exit"
  exit 4
fi
BUF=crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs
committed=$(git -C "$SRC" rev-parse "${FIXED}:${BUF}")
archived=$(git -C "$SRC" hash-object "$SNAP/$BUF")
arrow_stream=$(git -C "$SRC" rev-parse "${ARROW}:crates/oo-connectors/src/stream.rs")
this_stream=$(git -C "$SRC" rev-parse "${FIXED}:crates/oo-connectors/src/stream.rs")
test "$committed" = "$archived"
test "$arrow_stream" = "$this_stream"
shasum -a 256 "$SNAP/$BUF" | awk '{print $1}' > "$EVIDENCE/smoke/expect-buffered.sha256"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/smoke/source-tree.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/smoke/source-commit.txt"
printf 'git_blob=%s\narrow_stream_blob=%s\nfixed_stream_blob=%s\n' "$committed" "$arrow_stream" "$this_stream" > "$EVIDENCE/smoke/blob-identity.txt"
printf '%s\n' 0 > "$EVIDENCE/smoke/snapshot-prep.exit"

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/smoke/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/smoke/image.txt"

if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a135 \
    --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a135 \
    "$NET" > "$EVIDENCE/smoke/network.txt"
fi

docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology --label oo.owner=a135 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a135 --label oo.kind=cargo \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology --label oo.owner=a135 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a135 --label oo.kind=target \
  "$TARGET_VOL" >/dev/null

if docker container inspect "$NAME" >/dev/null 2>&1; then
  prior=$(docker inspect -f '{{.State.Status}}' "$NAME")
  printf 'prior_status=%s\n' "$prior" > "$EVIDENCE/smoke/prior-container.txt"
  if [ "$prior" = "running" ]; then
    echo "deferred: named container is already running" >&2
    printf '%s\n' 3 > "$EVIDENCE/smoke/docker-run.exit"
    exit 3
  fi
  docker rm "$NAME" >> "$EVIDENCE/smoke/prior-container.txt"
fi

cat > "$EVIDENCE/smoke/invocation.txt" <<EOF
source_commit=${FIXED}
tree=$(cat "$EVIDENCE/smoke/source-tree.txt")
arrow_commit=${ARROW}
snapshot_method=git archive of product paths from ${FIXED}
paths=Cargo.toml Cargo.lock rust-toolchain.toml crates .cargo contracts/source/pgsql/empty-catalog.json
empty_catalog_sha256=${FIXTURE_HASH}
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
container=${NAME}
network=${NET}
pull=never
privileged=no
home_mount=no
socket_mount=no
host_network=no
shared_cargo_cache=no
phases=clippy-component,fetch,lib,clippy
probes=not_in_this_container
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
  --mount "type=bind,source=${EVIDENCE}/smoke/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a135 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a135 \
  --label oo.task=init-tail-review-smoke \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/smoke/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/smoke/docker-run.exit"
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
} > "$EVIDENCE/smoke/hostconfig.txt"
exit "$run_exit"
