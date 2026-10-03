#!/bin/bash
# One owned linux/arm64 Rust 1.94 builder. Immutable git archive of 79515fcf.
# No PostgreSQL, no host network, no shared cargo home, no 512-frame run.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a132/SP03/view-shrink-review
ROOT=/tmp/oo-a132-eef831aa
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a132-view-shrink-review
NET=oo-a132-eef831aa-net
CARGO_VOL=oo-a132-eef831aa-cargo
TARGET_VOL=oo-a132-eef831aa-target
SOURCE=79515fcf7e7032c8c212540eaae4b48a5957e3a2
SNAP=${ROOT}/snapshot

mkdir -p "$EVIDENCE" "$ROOT"
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"

running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a132 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$((running_rust + host_cargo))" -ge 2 ]; then
  echo "deferred: rust builder capacity is full" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  echo "HOST_FAILED capacity"
  exit 3
fi

git -C "$SRC" rev-parse "$SOURCE" > "$EVIDENCE/source-commit.txt"
git -C "$SRC" rev-parse "${SOURCE}^{tree}" > "$EVIDENCE/source-tree.txt"
git -C "$SRC" rev-parse "${SOURCE}^" > "$EVIDENCE/source-parent.txt"
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/worktree-head.txt"
{
  cd "$SRC"
  shasum -a 256 \
    crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs \
    crates/oo-connectors/vendor/PATCH.md \
    crates/oo-connectors/tests/pg_wire_view_shrink.rs \
    crates/oo-connectors/vendor/sqlx-core/src/io/read_buf.rs \
    crates/oo-connectors/vendor/sqlx-core/src/net/tls/tls_rustls.rs \
    crates/oo-connectors/vendor/sqlx-postgres/src/connection/stream.rs
} > "$EVIDENCE/host-source.sha256"

rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive --format=tar "$SOURCE" \
  Cargo.toml Cargo.lock rust-toolchain.toml crates .cargo \
  | tar -C "$SNAP" -xf -
cmp -s \
  "$SRC/crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs" \
  "$SNAP/crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs"
grep -q 'saturating_sub(view_before)' "$SNAP/crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs"
if grep -q 'incoming view shrank' "$SNAP/crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs"; then
  echo "archive still has the panic expect" >&2
  echo "HOST_FAILED archive"
  exit 4
fi
printf '%s\n' 0 > "$EVIDENCE/snapshot-prep.exit"

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"

if docker container inspect "$NAME" >/dev/null 2>&1; then
  prior=$(docker inspect -f '{{.State.Status}}' "$NAME")
  if [ "$prior" = "running" ]; then
    echo "deferred: named container is already running" >&2
    printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
    echo "HOST_FAILED named-container"
    exit 3
  fi
  docker rm "$NAME" >/dev/null
fi

if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a132 \
    --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a132 \
    "$NET" >/dev/null
fi
docker volume create --label oo.project=open-ontology --label oo.owner=a132 --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a132 "$CARGO_VOL" >/dev/null
docker volume create --label oo.project=open-ontology --label oo.owner=a132 --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a132 "$TARGET_VOL" >/dev/null

cat > "$EVIDENCE/invocation.txt" <<EOF
source_commit=${SOURCE}
source_tree=$(cat "$EVIDENCE/source-tree.txt")
source_parent=$(cat "$EVIDENCE/source-parent.txt")
worktree_head=$(cat "$EVIDENCE/worktree-head.txt")
snapshot_method=git archive of ${SOURCE} only
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
msrv_override=RUSTUP_TOOLCHAIN=1.94.0
network=${NET}
network_mode=bridge_not_host
container=${NAME}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
snapshot=${SNAP}
privileged=no
home_mount=no
socket_mount=no
host_port=none
shared_cargo_cache=no
pull=never
cap_drop=ALL
cap_add=none
init=yes
no_new_privileges=yes
memory=8g
memory_swap=8g
cpus=4
pids_limit=4096
user=0:0
postgres=no
probe_frames=36
probe_chunks=4
probe_oversize=100000
not_the_512_frame_run=yes
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
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a132 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a132 \
  --label oo.task=view-shrink-review \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /evidence/inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Status=$(docker inspect -f '{{.State.Status}}' "$NAME" 2>/dev/null || echo missing)"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME" 2>/dev/null || echo missing)"
  echo "MemorySwap=$(docker inspect -f '{{.HostConfig.MemorySwap}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME" 2>/dev/null || echo missing)"
  echo "PidsLimit=$(docker inspect -f '{{.HostConfig.PidsLimit}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapDrop=$(docker inspect -f '{{.HostConfig.CapDrop}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapAdd=$(docker inspect -f '{{.HostConfig.CapAdd}}' "$NAME" 2>/dev/null || echo missing)"
  echo "SecurityOpt=$(docker inspect -f '{{.HostConfig.SecurityOpt}}' "$NAME" 2>/dev/null || echo missing)"
  docker inspect -f '{{range .Mounts}}mount dest={{.Destination}} type={{.Type}} rw={{.RW}}{{println}}{{end}}' "$NAME" 2>/dev/null || true
} > "$EVIDENCE/hostconfig.txt"
docker rm "$NAME" > "$EVIDENCE/container-rm.txt" 2>/dev/null || true
echo "HOST_DONE docker_run_exit=${run_exit}"
exit "$run_exit"
