#!/bin/bash
# One owned linux/arm64 Rust 1.94 builder. Archive of fixed product ccd plus the owned overlay.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a129/SP03/view-shrink/green
ROOT=/tmp/oo-a129-eef831aa
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a129-view-shrink-green
NET=oo-a129-eef831aa-net
CARGO_VOL=oo-a129-eef831aa-cargo
TARGET_VOL=oo-a129-eef831aa-target
BASE=ccd93e1adc34b24ab998d90972549f8cffb59f0d
SNAP=${ROOT}/green-snapshot

mkdir -p "$EVIDENCE" "$ROOT"
cp "$EVIDENCE/inside.sh" "$ROOT/inside-green.sh"
chmod 755 "$ROOT/inside-green.sh" "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"

running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a129 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$((running_rust + host_cargo))" -ge 2 ]; then
  echo "deferred: rust builder capacity is full" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi

git -C "$SRC" rev-parse HEAD > "$EVIDENCE/worktree-head.txt"
git -C "$SRC" rev-parse "${BASE}^{tree}" > "$EVIDENCE/archive-tree.txt"
git -C "$SRC" rev-parse "${BASE}^" > "$EVIDENCE/archive-parent.txt"
git -C "$SRC" diff --stat "$BASE" HEAD -- crates Cargo.toml Cargo.lock rust-toolchain.toml .cargo > "$EVIDENCE/product-drift.txt"
(
  cd "$SRC"
  shasum -a 256 \
    crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs \
    crates/oo-connectors/vendor/PATCH.md \
    crates/oo-connectors/tests/pg_wire_view_shrink.rs
) > "$EVIDENCE/green-source.sha256"

rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive --format=tar "$BASE" \
  Cargo.toml Cargo.lock rust-toolchain.toml crates .cargo \
  contracts/source/pgsql/empty-catalog.json \
  | tar -C "$SNAP" -xf -
overlay_buffered="$SRC/crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs"
overlay_patch="$SRC/crates/oo-connectors/vendor/PATCH.md"
overlay_test="$SRC/crates/oo-connectors/tests/pg_wire_view_shrink.rs"
cp "$overlay_buffered" "$SNAP/crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs"
cp "$overlay_patch" "$SNAP/crates/oo-connectors/vendor/PATCH.md"
cp "$overlay_test" "$SNAP/crates/oo-connectors/tests/pg_wire_view_shrink.rs"
cmp -s "$overlay_buffered" "$SNAP/crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs"
cmp -s "$overlay_patch" "$SNAP/crates/oo-connectors/vendor/PATCH.md"
cmp -s "$overlay_test" "$SNAP/crates/oo-connectors/tests/pg_wire_view_shrink.rs"
grep -q 'saturating_sub(view_before)' "$SNAP/crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs"
if grep -q 'incoming view shrank' "$SNAP/crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs"; then
  echo "overlay still has the panic expect" >&2
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
    exit 3
  fi
  docker rm "$NAME" >/dev/null
fi

cat > "$EVIDENCE/invocation.txt" <<EOF
archive_base=${BASE}
archive_tree=$(cat "$EVIDENCE/archive-tree.txt")
archive_parent=$(cat "$EVIDENCE/archive-parent.txt")
worktree_head=$(cat "$EVIDENCE/worktree-head.txt")
overlay=buffered.rs PATCH.md tests/pg_wire_view_shrink.rs
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
snapshot_method=git archive of ccd plus owned overlay
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
phase=green
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
  --mount "type=bind,source=${SNAP},target=/src" \
  --mount "type=bind,source=${ROOT}/inside-green.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a129 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a129 \
  --label oo.task=view-shrink-green \
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
  echo "MemorySwap=$(docker inspect -f '{{.HostConfig.MemorySwap}}' "$NAME")"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME")"
  echo "PidsLimit=$(docker inspect -f '{{.HostConfig.PidsLimit}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{.HostConfig.CapDrop}}' "$NAME")"
  echo "CapAdd=$(docker inspect -f '{{.HostConfig.CapAdd}}' "$NAME")"
  echo "SecurityOpt=$(docker inspect -f '{{.HostConfig.SecurityOpt}}' "$NAME")"
  docker inspect -f '{{range .Mounts}}mount dest={{.Destination}} type={{.Type}} rw={{.RW}}{{println}}{{end}}' "$NAME"
} > "$EVIDENCE/hostconfig.txt"
docker rm "$NAME" > "$EVIDENCE/container-rm.txt"
exit "$run_exit"
