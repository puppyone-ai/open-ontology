#!/bin/bash
# Immutable archive of cc764cbc. One rust builder. No PostgreSQL. No overlay.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
BASE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a159/SP04/full-import-review
EVIDENCE=${BASE}/linux/smoke
ROOT=/tmp/oo-a159-eef831aa-dba4-4af2-a78b-541bc9e0a159
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a159-smoke
NET=oo-a159-eef831aa-net
CARGO_VOL=oo-a159-eef831aa-cargo
TARGET_VOL=oo-a159-eef831aa-target
FIXED=cc764cbc5f9d5d2cf6056a2c41ff2612161b5ff4
EXPECT_TREE=da85327b402378cfdc3b826300205569c43b3642
SNAP=${ROOT}/smoke-src

mkdir -p "$EVIDENCE" "$ROOT/ca"
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite smoke console.log" >&2
  exit 2
fi
head_now=$(git -C "$SRC" rev-parse HEAD)
printf '%s\n' "$head_now" > "$EVIDENCE/workspace-head.txt"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  exit 1
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a159 \
    --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a159 \
    "$NET" > "$EVIDENCE/network.txt"
fi
docker network inspect --format '{{.Id}} {{.Driver}}' "$NET" > "$EVIDENCE/network.txt"
docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}' > "$EVIDENCE/containers-before.txt"
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a159 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$((running_rust + host_cargo))" -ge 2 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
actual_tree=$(git -C "$SRC" rev-parse "${FIXED}^{tree}")
test "$actual_tree" = "$EXPECT_TREE"
printf '%s\n' "$FIXED" > "$EVIDENCE/archive-base.txt"
printf '%s\n' "$actual_tree" > "$EVIDENCE/archive-tree.txt"
rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive "$FIXED" | tar -x -C "$SNAP"
if [ -e "$SNAP/crates/oo-pipeline/tests/full_import_a159_review.rs" ]; then
  echo "archive already contains the review overlay" >&2
  exit 2
fi
file_sha() { shasum -a 256 "$1" | awk '{print $1}'; }
file_sha "$SNAP/crates/oo-pipeline/Cargo.toml" > "$EVIDENCE/expect-cargo.sha256"
file_sha "$SNAP/crates/oo-pipeline/src/lib.rs" > "$EVIDENCE/expect-lib.sha256"
file_sha "$SNAP/crates/oo-pipeline/src/full_import.rs" > "$EVIDENCE/expect-full-import.sha256"
file_sha "$SNAP/crates/oo-pipeline/src/copy_sink.rs" > "$EVIDENCE/expect-sink.sha256"
file_sha "$SNAP/crates/oo-pipeline/src/copy_encoder.rs" > "$EVIDENCE/expect-encoder.sha256"
file_sha "$SNAP/crates/oo-pipeline/src/physical_mapping.rs" > "$EVIDENCE/expect-mapping.sha256"
file_sha "$SNAP/crates/oo-pipeline/src/target_installer.rs" > "$EVIDENCE/expect-installer.sha256"
file_sha "$SNAP/crates/oo-pipeline/migrations/0005_full_import_expand.sql" > "$EVIDENCE/expect-0005.sha256"
file_sha "$SNAP/contracts/pipeline/full-import-v1.json" > "$EVIDENCE/expect-contract-json.sha256"
{
  echo "full_import $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-pipeline/src/full_import.rs")"
  echo "copy_sink $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-pipeline/src/copy_sink.rs")"
  echo "installer $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-pipeline/src/target_installer.rs")"
  echo "lib $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-pipeline/src/lib.rs")"
  echo "encoder $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-pipeline/src/copy_encoder.rs")"
  echo "mapping $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-pipeline/src/physical_mapping.rs")"
  echo "sql0000 $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-pipeline/migrations/0000_bootstrap_roles.sql")"
  echo "sql0001 $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-pipeline/migrations/0001_target_fence_expand.sql")"
  echo "sql0002 $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-pipeline/migrations/0002_target_segment_null_expand.sql")"
  echo "sql0003 $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-pipeline/migrations/0003_target_segment_preflight_expand.sql")"
  echo "sql0004 $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-pipeline/migrations/0004_physical_mapping.sql")"
  echo "sql0005 $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-pipeline/migrations/0005_full_import_expand.sql")"
  echo "contract $(git -C "$SRC" rev-parse "${FIXED}:contracts/pipeline/full-import-v1.json")"
  echo "lock $(git -C "$SRC" rev-parse "${FIXED}:Cargo.lock")"
} > "$EVIDENCE/fixed-blobs.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a159 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a159 \
  --label oo.kind=cargo \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a159 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a159 \
  --label oo.kind=target \
  "$TARGET_VOL" >/dev/null
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named smoke container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
cat > "$EVIDENCE/invocation.txt" <<EOF
workspace_head=${head_now}
fixed=${FIXED}
tree=${EXPECT_TREE}
composition=git archive, no overlay
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
host_ca_mounted=no
private_ca=${ROOT}/ca
phases=toolchain fetch lib clippy classifier
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
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${ROOT}/ca,target=/private-ca,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a159 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a159 \
  --label oo.task=full-import-review-smoke \
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
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
  echo "CapAdd=$(docker inspect -f '{{json .HostConfig.CapAdd}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "PortBindings=$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$EVIDENCE/hostconfig.txt"
exit "$run_exit"
